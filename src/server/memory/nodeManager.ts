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
import { TOOL_NAMES } from "@/shared/constants.ts";

export interface NodePropertyDef {
  name: string;
  description: string;
  type?: string;
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
    type: "string",
  },
];

const EMBEDDING_PROP: NodePropertyDef = {
  name: "_embedding",
  description: "Internal vector embedding for semantic search. Hidden from GM tools.",
  type: "number[]",
};

const ENTITY_PROPS: NodePropertyDef[] = [
  { name: "name", description: "Unique name of the entity.", type: "string" },
  {
    name: "type",
    description: "Entity type: CHARACTER, OBJECT, LOCATION, ORGANIZATION, or EVENT.",
    type: "string",
  },
  {
    name: "subtype",
    description: "Optional subtype refinement (e.g., 'Weapon' for an OBJECT).",
    type: "string",
  },
  { name: "description", description: "Full narrative description of the entity.", type: "string" },
  { name: "brief", description: "One-line summary for compact display.", type: "string" },
  {
    name: "metadata",
    description:
      "JSON object: stats (skill→value), conditions, attributes (key→description), opinions (target→text), aliases (string[]).",
    type: "json",
  },
  { name: "created_at", description: "ISO 8601 timestamp of creation.", type: "string" },
];

const TIMESTAMP_PROPS: NodePropertyDef[] = [
  { name: "created_at", description: "ISO 8601 timestamp of creation.", type: "string" },
  { name: "updated_at", description: "ISO 8601 timestamp of last update.", type: "string" },
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
        type: "string",
      },
      { name: "created_at", description: "ISO 8601 timestamp of creation.", type: "string" },
      { name: "updated_at", description: "ISO 8601 timestamp of last update.", type: "string" },
      {
        name: "options",
        description: "JSON array of current dialogue options for session resume.",
        type: "json",
      },
      ...INTERNAL_PROPS,
    ],
  },
  {
    name: "GMTurnMessage",
    description:
      "Stores AI SDK messages for multi-turn GM continuity. Internal bookkeeping — not visible to GM.",
    properties: [
      { name: "turn", description: "Turn number for this message group.", type: "number" },
      {
        name: "messages",
        description: "JSON array of serialized AI SDK ModelMessage objects.",
        type: "json",
      },
      {
        name: "user_input",
        description: "The player input that triggered this turn.",
        type: "string",
      },
      { name: "created_at", description: "ISO 8601 timestamp of creation.", type: "string" },
      ...INTERNAL_PROPS,
    ],
  },
  {
    name: "IdCounter",
    description: "Atomic counter for generating short message IDs. Internal bookkeeping.",
    properties: [
      { name: "session_id", description: "Fixed session key for the counter.", type: "string" },
      { name: "counter", description: "Current counter value (Neo4j Integer).", type: "number" },
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
    properties: ENTITY_PROPS,
  },
  {
    name: "Object",
    description: "Dynamic sub-label of Entity for OBJECT type. Inherits all Entity properties.",
    properties: ENTITY_PROPS,
  },
  {
    name: "Location",
    description: "Dynamic sub-label of Entity for LOCATION type. Inherits all Entity properties.",
    properties: ENTITY_PROPS,
  },
  {
    name: "Organization",
    description:
      "Dynamic sub-label of Entity for ORGANIZATION type. Inherits all Entity properties.",
    properties: ENTITY_PROPS,
  },
  {
    name: "Event",
    description: "Dynamic sub-label of Entity for EVENT type. Inherits all Entity properties.",
    properties: ENTITY_PROPS,
  },
  {
    name: "Message",
    description:
      "A conversation message between player and GM. Linked in sequence via NEXT_MESSAGE.",
    properties: [
      { name: "role", description: "Message role: user, assistant, or system.", type: "string" },
      { name: "content", description: "Message text content.", type: "string" },
      { name: "timestamp", description: "ISO 8601 timestamp of the message.", type: "string" },
      {
        name: "metadata",
        description:
          "JSON object with speaker (voice name), type (NARRATION/CHARACTER/SYSTEM/ROLL).",
        type: "json",
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
      { name: "name", description: "Unique note name (used as lookup key).", type: "string" },
      {
        name: "content",
        description: "Full note content (embedded for vector search).",
        type: "string",
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
      { name: "name", description: "Unique plot name (used as lookup key).", type: "string" },
      {
        name: "description",
        description: "Full plot description (embedded for vector search).",
        type: "string",
      },
      { name: "brief", description: "One-line plot summary for compact display.", type: "string" },
      {
        name: "status",
        description: "Plot lifecycle: PENDING, ACTIVE, IN_PROGRESS, COMPLETED, or ABANDONED.",
        type: "string",
      },
      {
        name: "trigger_condition",
        description: "JS expression evaluated to auto-activate the plot.",
        type: "string",
      },
      {
        name: "flags",
        description: "JSON array of {flagId, description} tracking plot milestones.",
        type: "json",
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
        type: "string",
      },
      {
        name: "target_name",
        description: "Name of the target entity this disposition is about.",
        type: "string",
      },
      {
        name: "sentiment",
        description:
          "Sentiment label: protective, trusting, fearful, hostile, attracted, suspicious, resentful, grateful, or indifferent.",
        type: "string",
      },
      {
        name: "summary",
        description: "Human-readable summary of the disposition and its context.",
        type: "string",
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
      { name: "day", description: "In-game day number (starts at 1).", type: "number" },
      {
        name: "segment",
        description: "Segment within the day (0–11, each = 2 hours).",
        type: "number",
      },
      {
        name: "label",
        description:
          "Human-readable label: Midnight, Dawn, Morning, Noon, Afternoon, Dusk, Night, etc.",
        type: "string",
      },
      { name: "created_at", description: "ISO 8601 timestamp of creation.", type: "string" },
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
    name: "GameTime",
    description: "Legacy game time node — migrated to TimeAnchor/TimePoint system on startup.",
    properties: [
      { name: "day", description: "In-game day number.", type: "number" },
      { name: "segment", description: "Segment 0–11.", type: "number" },
      ...INTERNAL_PROPS,
    ],
  },
  {
    name: "RelationshipType",
    description:
      `Stores the description and category of each relationship type in the schema. Use ${TOOL_NAMES.MANAGE_SCHEMA} to register new types.`,
    properties: [
      {
        name: "name",
        description: "Relationship type name (e.g. 'LOCATED_AT', 'CONNECTED_TO').",
        type: "string",
      },
      {
        name: "description",
        description: "Human-readable description of what the relationship means.",
        type: "string",
      },
      { name: "category", description: "INTERNAL, PREDEFINED, or GM_DEFINED.", type: "string" },
    ],
  },
  {
    name: "NodeType",
    description:
      `Stores the description, property schema, and category of each node type in the schema. Use ${TOOL_NAMES.MANAGE_SCHEMA} to register new types.`,
    properties: [
      { name: "name", description: "Node label (e.g. 'Entity', 'Artifact').", type: "string" },
      {
        name: "description",
        description: "Human-readable description of what the node type represents.",
        type: "string",
      },
      { name: "category", description: "INTERNAL, PREDEFINED, or GM_DEFINED.", type: "string" },
      {
        name: "properties",
        description:
          "JSON array of {name, description, type} describing the node's property schema.",
        type: "json",
      },
    ],
  },
];

// PREDEFINED labels that are readable via queryWorld but NOT writable via mutateWorld.
// The GM uses manageSchema to register/unregister these instead.
const WRITE_BLOCKED_NAMES = new Set(["RelationshipType", "NodeType"]);

export class NodeManager {
  private registry = new Map<string, NodeDef>();

  private constructor() {
    for (const t of INTERNAL_TYPES) {
      this.registry.set(t.name, {
        ...t,
        properties: [],
        type: "INTERNAL",
      });
    }
    for (const t of PREDEFINED_TYPES) {
      this.registry.set(t.name, {
        ...t,
        properties: [],
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
