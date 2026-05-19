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
import { TOOL_NAMES } from "@/shared/constants";

export const NODE_PROPERTY_TAGS = [
  "string",
  "number",
  "number[]",
  /**
   * Saved as string in Neo4j, actually. When use editNode to partial update, will automatically
   * unfold Neo4j string property to avoid whole string overwritten (which is unwanted in most cases).
   */
  "json",
  /**
   * Will be used to compute property `_embedding`.
   */
  "embedded",
  /**
   * Will create a unique constraint if specified.
   * `CREATE CONSTRAINT $name IF NOT EXISTS FOR (n:$label) REQUIRE n.$prop IS UNIQUE`
   */
  "unique",
  /**
   * Will create a regular index on specified property.
   * `CREATE INDEX $name IF NOT EXISTS FOR (n:$label) ON (n.$prop)`
   */
  "index",
  /**
   * Will create composite index on all specified properties.
   */
  "composite_index_1", // Composite index group 1 for a node type.
  "composite_index_2", // Composite index group 2 for a node type.
  "composite_index_3", // Composite index group 3 for a node type.
] as const;
export type NodePropertyTag = (typeof NODE_PROPERTY_TAGS)[number];

export interface NodePropertyDef {
  name: string;
  description: string;
  tags: NodePropertyTag[];
}

export interface NodeDef {
  name: string;
  description: string;
  properties: NodePropertyDef[];
  type: "INTERNAL" | "PREDEFINED" | "GM_DEFINED";
}

// ── Common property sets ──

const INTERNAL_PROPS: NodePropertyDef[] = [
  {
    name: "_id",
    description: "Internal unique identifier. Hidden from GM tools.",
    tags: ["string", "unique"],
  },
];

const EMBEDDING_PROP: NodePropertyDef = {
  name: "_embedding",
  description: "Internal vector embedding for semantic search. Hidden from GM tools.",
  tags: ["number[]"],
};

const TIMESTAMP_PROPS: NodePropertyDef[] = [
  { name: "_created_at", description: "ISO 8601 timestamp of creation.", tags: ["string"] },
  { name: "_updated_at", description: "ISO 8601 timestamp of last update.", tags: ["string"] },
];

const ENTITY_PROPS: NodePropertyDef[] = [
  { name: "name", description: "Unique name of the entity.", tags: ["string", "embedded", "index"] },
  {
    name: "type",
    description: "Entity type: CHARACTER, OBJECT, LOCATION, ORGANIZATION, or EVENT.",
    tags: ["string", "index"],
  },
  {
    name: "subtype",
    description: "Optional subtype refinement (e.g., 'Weapon' for an OBJECT).",
    tags: ["string"],
  },
  {
    name: "description",
    description: "Full narrative description of the entity.",
    tags: ["string", "embedded"],
  },
  { name: "brief", description: "One-line summary for compact display.", tags: ["string", "embedded"] },
  {
    name: "metadata",
    description:
      "JSON object: stats (skill→value), conditions, attributes (key→description), opinions (target→text), aliases (string[]).",
    tags: ["json"],
  },
  { name: "_created_at", description: "ISO 8601 timestamp of creation.", tags: ["string"] },
];

const INTERNAL_TYPES: { name: string; description: string; properties: NodePropertyDef[] }[] = [
  {
    name: "Conversation",
    description:
      "Singleton node storing the game session. Internal bookkeeping — not visible to GM.",
    properties: [
      {
        name: "session_id",
        description: "Fixed game session key ('chorus-game').",
        tags: ["string", "unique"],
      },
      { name: "_created_at", description: "ISO 8601 timestamp of creation.", tags: ["string"] },
      { name: "_updated_at", description: "ISO 8601 timestamp of last update.", tags: ["string"] },
      {
        name: "options",
        description: "JSON array of current dialogue options for session resume.",
        tags: ["json"],
      },
      ...INTERNAL_PROPS,
    ],
  },
  {
    name: "GMTurnMessage",
    description:
      "Stores AI SDK messages for multi-turn GM continuity. Internal bookkeeping — not visible to GM.",
    properties: [
      { name: "turn", description: "Turn number for this message group.", tags: ["number"] },
      {
        name: "messages",
        description: "JSON array of serialized AI SDK ModelMessage objects.",
        tags: ["json"],
      },
      {
        name: "user_input",
        description: "The player input that triggered this turn.",
        tags: ["string"],
      },
      { name: "_created_at", description: "ISO 8601 timestamp of creation.", tags: ["string"] },
      ...INTERNAL_PROPS,
    ],
  },
  {
    name: "IdCounter",
    description: "Atomic counter for generating short message IDs. Internal bookkeeping.",
    properties: [
      { name: "session_id", description: "Fixed session key for the counter.", tags: ["string", "unique"] },
      { name: "counter", description: "Current counter value (Neo4j Integer).", tags: ["number"] },
    ],
  },
];

const PREDEFINED_TYPES: { name: string; description: string; properties: NodePropertyDef[] }[] = [
  {
    name: "Entity",
    description:
      "A world entity (CHARACTER, OBJECT, LOCATION, ORGANIZATION, or EVENT). Core building block of the world model.",
    properties: [...ENTITY_PROPS, EMBEDDING_PROP, ...INTERNAL_PROPS],
  },
  {
    name: "Character",
    description: "Dynamic sub-label of Entity for CHARACTER type. Inherits all Entity properties.",
    properties: [...ENTITY_PROPS, EMBEDDING_PROP, ...INTERNAL_PROPS],
  },
  {
    name: "Object",
    description: "Dynamic sub-label of Entity for OBJECT type. Inherits all Entity properties.",
    properties: [...ENTITY_PROPS, EMBEDDING_PROP, ...INTERNAL_PROPS],
  },
  {
    name: "Location",
    description: "Dynamic sub-label of Entity for LOCATION type. Inherits all Entity properties.",
    properties: [...ENTITY_PROPS, EMBEDDING_PROP, ...INTERNAL_PROPS],
  },
  {
    name: "Message",
    description:
      "A conversation message between player and GM. Linked in sequence via NEXT_MESSAGE.",
    properties: [
      { name: "content", description: "Message text content.", tags: ["string", "embedded"] },
      { name: "timestamp", description: "ISO 8601 timestamp of the message.", tags: ["string"] },
      {
        name: "metadata",
        description:
          "JSON object including speaker (voice name), and type (CHARACTER/SYSTEM/ROLL/INNER_VOICE).",
        tags: ["json"],
      },
      EMBEDDING_PROP,
      ...INTERNAL_PROPS,
    ],
  },
  {
    name: "Note",
    description:
      "A GM note with vector embedding for semantic recall. Can link to Entities or Messages via ABOUT_ENTITY / ABOUT_MESSAGE.",
    properties: [
      { name: "name", description: "Unique note name (used as lookup key).", tags: ["string"] },
      {
        name: "content",
        description: "Full note content (embedded for vector search).",
        tags: ["string", "embedded"],
      },
      ...TIMESTAMP_PROPS,
      EMBEDDING_PROP,
      ...INTERNAL_PROPS,
    ],
  },
  {
    name: "Plot",
    description:
      "A narrative plot with status, beats, branches, and flags. Drives story progression.",
    properties: [
      { name: "name", description: "Unique plot name (used as lookup key).", tags: ["string", "unique", "embedded"] },
      {
        name: "description",
        description: "Full plot description (embedded for vector search).",
        tags: ["string", "embedded"],
      },
      {
        name: "brief",
        description: "One-line plot summary for compact display.",
        tags: ["string", "embedded"],
      },
      {
        name: "status",
        description: "Plot lifecycle: PENDING, ACTIVE, IN_PROGRESS, COMPLETED, or ABANDONED.",
        tags: ["string"],
      },
      {
        name: "trigger_condition",
        description: "JS expression evaluated to auto-activate the plot.",
        tags: ["string"],
      },
      {
        name: "flags",
        description: "JSON array of {flagId, description} tracking plot milestones.",
        tags: ["json"],
      },
      ...TIMESTAMP_PROPS,
      EMBEDDING_PROP,
      ...INTERNAL_PROPS,
    ],
  },
  {
    name: "NPCDisposition",
    description:
      "An NPC's sentiment and summary toward a target entity. Stored as a NODE (not a relationship). Match via (npc:Entity)-[:HAS_DISPOSITION]->(d:NPCDisposition {target_name: '...'}).",
    properties: [
      {
        name: "npc_name",
        description: "Name of the NPC entity who holds this disposition.",
        tags: ["string", "index", "composite_index_1"],
      },
      {
        name: "target_name",
        description: "Name of the target entity this disposition is about.",
        tags: ["string", "index", "composite_index_1"],
      },
      {
        name: "sentiment",
        description:
          "Sentiment label: protective, trusting, fearful, hostile, attracted, suspicious, resentful, grateful, or indifferent.",
        tags: ["string"],
      },
      {
        name: "summary",
        description: "Human-readable summary of the disposition and its context.",
        tags: ["string"],
      },
      ...TIMESTAMP_PROPS,
      ...INTERNAL_PROPS,
    ],
  },
  {
    name: "TimePoint",
    description:
      "A point in game time with day, segment, and label. Linked sequentially via NEXT_TIMEPOINT.",
    properties: [
      { name: "day", description: "In-game day number (starts at 1).", tags: ["number", "composite_index_1"] },
      {
        name: "segment",
        description: "Segment within the day (0–11, each = 2 hours).",
        tags: ["number", "composite_index_1"],
      },
      {
        name: "label",
        description:
          "Human-readable label: Midnight, Dawn, Morning, Noon, Afternoon, Dusk, Night, etc.",
        tags: ["string"],
      },
      { name: "_created_at", description: "ISO 8601 timestamp of creation.", tags: ["string"] },
      ...INTERNAL_PROPS,
    ],
  },
  {
    name: "TimeAnchor",
    description:
      "Singleton anchor pointing to the current TimePoint via CURRENT_TIMEPOINT. Use MATCH (a:TimeAnchor {_id:'anchor'})-[:CURRENT_TIMEPOINT]->(tp:TimePoint) to get current time.",
    properties: [...INTERNAL_PROPS],
  },
  {
    name: "RelationshipType",
    description: `Stores the description and category of each relationship type in the schema. Use ${TOOL_NAMES.MANAGE_SCHEMA} to register new types.`,
    properties: [
      {
        name: "name",
        description: "Relationship type name (e.g. 'LOCATED_AT', 'CONNECTED_TO').",
        tags: ["string"],
      },
      {
        name: "description",
        description: "Human-readable description of what the relationship means.",
        tags: ["string"],
      },
      { name: "category", description: "INTERNAL, PREDEFINED, or GM_DEFINED.", tags: ["string"] },
    ],
  },
  {
    name: "NodeType",
    description: `Stores the description, property schema, and category of each node type in the schema. Use ${TOOL_NAMES.MANAGE_SCHEMA} to register new types.`,
    properties: [
      { name: "name", description: "Node label (e.g. 'Entity', 'Artifact').", tags: ["string"] },
      {
        name: "description",
        description: "Human-readable description of what the node type represents.",
        tags: ["string"],
      },
      { name: "category", description: "INTERNAL, PREDEFINED, or GM_DEFINED.", tags: ["string"] },
      {
        name: "properties",
        description:
          "JSON array of {name, description, type} describing the node's property schema.",
        tags: ["json"],
      },
    ],
  },
];

// PREDEFINED labels that are readable via queryWorld but NOT writable via queryWorld (WRITE action).
// The GM uses manageSchema to register/unregister these instead.
const WRITE_BLOCKED_NAMES = new Set(["RelationshipType", "NodeType"]);

export class NodeManager {
  private registry = new Map<string, NodeDef>();

  private constructor() {
    for (const t of INTERNAL_TYPES) {
      this.registry.set(t.name, {
        ...t,
        type: "INTERNAL",
      });
    }
    for (const t of PREDEFINED_TYPES) {
      this.registry.set(t.name, {
        ...t,
        type: "PREDEFINED",
      });
    }
  }

  register(
    name: string,
    description: string,
    properties: NodePropertyDef[] = [],
    type: "INTERNAL" | "PREDEFINED" | "GM_DEFINED",
  ): void {
    const existing = this.registry.get(name);
    if (existing) {
      if (existing.type !== type) {
        console.warn(
          `[NodeManager] "${name}" already registered as ${existing.type}, ignoring re-registration as ${type}`,
        );
      }
      return;
    }
    this.registry.set(name, { name, description, properties, type });
  }

  unregister(name: string): boolean {
    const def = this.registry.get(name);
    if (!def || def.type !== "GM_DEFINED") return false;
    this.registry.delete(name);
    return true;
  }

  get(name: string): NodeDef | undefined {
    return this.registry.get(name);
  }

  getAll(): NodeDef[] {
    return [...this.registry.values()];
  }

  getByType(type: "INTERNAL" | "PREDEFINED" | "GM_DEFINED"): NodeDef[] {
    return [...this.registry.values()].filter((r) => r.type === type);
  }

  /** Build embedding text by concatenating all "embedded"-tagged property values. */
  getEmbeddingText(label: string, props: Record<string, unknown>): string {
    const def = this.registry.get(label);
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

  isAllowedForRead(name: string): boolean {
    return this.registry.has(name);
  }

  isAllowedForWrite(name: string): boolean {
    const def = this.registry.get(name);
    if (!def) return false;
    if (WRITE_BLOCKED_NAMES.has(name)) return false;
    return def.type === "PREDEFINED" || def.type === "GM_DEFINED";
  }

  // Update the description and/or property schema of a GM_DEFINED node type.
  updateDefinition(
    name: string,
    updates: { description?: string; properties?: NodePropertyDef[] },
  ): boolean {
    const def = this.registry.get(name);
    if (!def || def.type !== "GM_DEFINED") return false;
    if (updates.description !== undefined) def.description = updates.description;
    if (updates.properties !== undefined) def.properties = updates.properties;
    return true;
  }

  // Clear all GM_DEFINED types from the registry (keeps INTERNAL + PREDEFINED).
  reset(): void {
    for (const [name, def] of this.registry) {
      if (def.type === "GM_DEFINED") {
        this.registry.delete(name);
      }
    }
  }

  // Sync all registered node types to Neo4j as :NodeType nodes.
  async syncToNeo4j(client: Neo4jClient): Promise<void> {
    for (const def of this.registry.values()) {
      await client.executeWrite(
        `MERGE (nt:NodeType {name: $name})
         SET nt.description = $description,
             nt.category = $category,
             nt.properties = $properties`,
        {
          name: def.name,
          description: def.description,
          category: def.type,
          properties: def.properties.length > 0 ? JSON.stringify(def.properties) : null,
        },
      );

      // Create unique constraint for properties with tag "unique".
      const uniquePropNames = new Set(
        def.properties.filter(prop => prop.tags.includes("unique")).map(prop => prop.name),
      );
      for (const propName of uniquePropNames) {
        const constraintName = def.name.toLowerCase() + "_" + propName;
        try {
          await client.executeWrite(
            `CREATE CONSTRAINT ${constraintName} IF NOT EXISTS FOR (n:\`${def.name}\`) REQUIRE n.\`${propName}\` IS UNIQUE`,
          );
        } catch (err) {
          const msg = err instanceof Error ? err.message : String(err);
          // If a standalone index already exists on this property, Neo4j refuses to
          // create the constraint. Look up and drop the index, then retry.
          if (msg.includes("IndexAlreadyExists") || msg.includes("already exists an index")) {
            try {
              const rows = await client.executeRead(`SHOW INDEXES YIELD name, labelsOrTypes, properties RETURN name, labelsOrTypes, properties`);
              for (const row of rows) {
                const labels: string[] = row.labelsOrTypes as string[];
                const props: string[] = row.properties as string[];
                if (labels?.includes(def.name) && props?.includes(propName)) {
                  await client.executeWrite(`DROP INDEX \`${row.name}\``);
                }
              }
              await client.executeWrite(
                `CREATE CONSTRAINT ${constraintName} IF NOT EXISTS FOR (n:\`${def.name}\`) REQUIRE n.\`${propName}\` IS UNIQUE`,
              );
              console.log(`[syncToNeo4j] Dropped existing index on (:${def.name} {${propName}}), created unique constraint ${constraintName}`);
            } catch (retryErr) {
              const retryMsg = retryErr instanceof Error ? retryErr.message : String(retryErr);
              console.error(`[syncToNeo4j] Unique constraint on ${constraintName} not created after dropping index: ${retryMsg}`);
            }
          } else {
            console.error(`[syncToNeo4j] Unique constraint on ${constraintName} not created: ${msg}`);
          }
        }
      }

      // Create regular index for properties with tag "index" that do NOT also have "unique".
      // A unique constraint already provides an implicit index — creating a second index
      // on the same property would fail with IndexAlreadyExists.
      const propsWithIndexTag = def.properties
        .filter(prop => prop.tags.includes("index") && !uniquePropNames.has(prop.name))
        .map(prop => prop.name);
      for (const propName of propsWithIndexTag) {
        const constraintName = def.name.toLowerCase() + "_" + propName + "_idx";
        try {
          await client.executeWrite(`CREATE INDEX ${constraintName} IF NOT EXISTS FOR (n:\`${def.name}\`) ON (n.\`${propName}\`)`);
        } catch (err) {
          const msg = err instanceof Error ? err.message : String(err);
          console.error(`[syncToNeo4j] Regular index on ${constraintName} not created: ${msg}`);
        }
      }

      // Create composite index for all properties groups.
      for (const index of ["composite_index_1", "composite_index_2", "composite_index_3"]) {
        const props = def.properties
          .filter(prop => prop.tags.includes(index as NodePropertyTag))
          .map(prop => prop.name);
        if (props.length < 2) continue;
        const constraintName = `${def.name.toLowerCase()}_${props.join("_")}_idx${index.at(-1)}`;
        try {
          await client.executeWrite(`CREATE INDEX ${constraintName} IF NOT EXISTS FOR (n:${def.name}) ON (${props.map(name => "n." + name).join(", ")})`);
        } catch (err) {
          const msg = err instanceof Error ? err.message : String(err);
          console.error(`[syncToNeo4j] Composite index on ${constraintName} not created: ${msg}`);
        }
      }

      // Create vector indexes (require Neo4j 5.11+) for node type that has "_embedding" properties.
      if (def.properties.some(prop => prop.name === "_embedding")) {
        const vectorIndexName = def.name + "_embedding_idx";
        const dimensions = process.env.EMBEDDING_DIMENSIONS || 1024;
        try {
          await client.executeWrite(
            `CREATE VECTOR INDEX ${vectorIndexName} IF NOT EXISTS FOR (n:${def.name}) ON (n._embedding)
            OPTIONS { indexConfig: { \`vector.dimensions\`: ${dimensions}, \`vector.similarity_function\`: 'COSINE' } }`,
          );
        } catch {
          console.error(`[syncToNeo4j] Vector index ${vectorIndexName} not created (Neo4j 5.11+ required).`);
        }
      }
    }
  }

  // ── Singleton ──

  private static instance: NodeManager | null = null;

  static getCachedInstance(): NodeManager {
    if (!NodeManager.instance) {
      NodeManager.instance = new NodeManager();
    }
    return NodeManager.instance;
  }
}
