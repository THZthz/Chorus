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

export interface RelationshipDef {
  name: string;
  description: string;
  type: "INTERNAL" | "PREDEFINED" | "GM_DEFINED";
  sourceLabels?: string[];
  targetLabels?: string[];
}

const INTERNAL_TYPES: { name: string; description: string; sourceLabels?: string[]; targetLabels?: string[] }[] = [
  {
    name: "_HAS_GM_MESSAGE",
    description: "Links a Conversation node to its GMTurnMessage nodes.",
    sourceLabels: ["Conversation"],
    targetLabels: ["GMTurnMessage"],
  },
  {
    name: "_FIRST_GM_MESSAGE",
    description: "Points to the first GMTurnMessage in a Conversation's ordered linked list.",
    sourceLabels: ["Conversation"],
    targetLabels: ["GMTurnMessage"],
  },
  {
    name: "_NEXT_GM_MESSAGE",
    description: "Sequentially links GMTurnMessage nodes in conversation order.",
    sourceLabels: ["GMTurnMessage"],
    targetLabels: ["GMTurnMessage"],
  },
];

const PREDEFINED_TYPES: { name: string; description: string; sourceLabels?: string[]; targetLabels?: string[] }[] = [
  {
    name: "HAS_MESSAGE",
    description: "Links a Conversation node to its Message nodes.",
    sourceLabels: ["Conversation"],
    targetLabels: ["Message"],
  },
  {
    name: "FIRST_MESSAGE",
    description: "Points to the first Message in a Conversation's ordered linked list.",
    sourceLabels: ["Conversation"],
    targetLabels: ["Message"],
  },
  {
    name: "NEXT_MESSAGE",
    description: "Sequentially links Message nodes in conversation order.",
    sourceLabels: ["Message"],
    targetLabels: ["Message"],
  },
  {
    name: "NEXT_TIMEPOINT",
    description: "Links TimePoint nodes in chronological sequence.",
    sourceLabels: ["TimePoint"],
    targetLabels: ["TimePoint"],
  },
  {
    name: "CURRENT_TIMEPOINT",
    description: "Points to the current TimePoint from a TimeAnchor node.",
    sourceLabels: ["TimeAnchor"],
    targetLabels: ["TimePoint"],
  },
  {
    name: "AT_TIME",
    description: "Links a Message to the TimePoint when it was created.",
    sourceLabels: ["Message"],
    targetLabels: ["TimePoint"],
  },
  {
    name: "STARTED_AT",
    description: "Marks the TimePoint when a Plot started.",
    sourceLabels: ["Plot"],
    targetLabels: ["TimePoint"],
  },
  {
    name: "ACTIVE_AT",
    description: "Marks the TimePoint when a Plot became active.",
    sourceLabels: ["Plot"],
    targetLabels: ["TimePoint"],
  },
  {
    name: "COMPLETED_AT",
    description: "Marks the TimePoint when a Plot completed.",
    sourceLabels: ["Plot"],
    targetLabels: ["TimePoint"],
  },
  {
    name: "LOCATED_AT",
    description: "An entity is physically present at a location.",
    sourceLabels: ["Entity"],
    targetLabels: ["Location"],
  },
  {
    name: "CARRIES",
    description: "An entity is carrying or in possession of an object.",
    sourceLabels: ["Entity"],
    targetLabels: ["Object"],
  },
  {
    name: "ALLIED_WITH",
    description: "An entity is allied with or friendly toward another entity.",
    sourceLabels: ["Entity"],
    targetLabels: ["Entity"],
  },
  {
    name: "HOSTILE_TOWARDS",
    description: "An entity is hostile toward or in conflict with another entity.",
    sourceLabels: ["Entity"],
    targetLabels: ["Entity"],
  },
  {
    name: "LOCATED_IN",
    description: "A location or entity is contained within a larger location.",
    sourceLabels: ["Entity"],
    targetLabels: ["Location"],
  },
  {
    name: "HAS_DISPOSITION",
    description: "Links an Entity (NPC) to its NPCDisposition node.",
    sourceLabels: ["Entity"],
    targetLabels: ["NPCDisposition"],
  },
  {
    name: "ABOUT_ENTITY",
    description: "A Note is about or references an Entity.",
    sourceLabels: ["Note"],
    targetLabels: ["Entity"],
  },
  {
    name: "ABOUT_MESSAGE",
    description: "A Note is about or references a specific Message.",
    sourceLabels: ["Note"],
    targetLabels: ["Message"],
  },
  {
    name: "BRANCHES_TO",
    description: "A parent Plot branches to a child sub-plot.",
    sourceLabels: ["Plot"],
    targetLabels: ["Plot"],
  },
];

export class RelationshipManager {
  private registry = new Map<string, RelationshipDef>();

  private constructor() {
    for (const t of INTERNAL_TYPES) {
      this.registry.set(t.name, { ...t, type: "INTERNAL" });
    }
    for (const t of PREDEFINED_TYPES) {
      this.registry.set(t.name, { ...t, type: "PREDEFINED" });
    }
  }

  register(
    name: string,
    description: string,
    type: "INTERNAL" | "PREDEFINED" | "GM_DEFINED",
    sourceLabels?: string[],
    targetLabels?: string[],
  ): void {
    const existing = this.registry.get(name);
    if (existing) {
      if (existing.type !== type) {
        console.warn(
          `[RelationshipManager] "${name}" already registered as ${existing.type}, ignoring re-registration as ${type}`,
        );
      }
      return;
    }
    this.registry.set(name, { name, description, type, sourceLabels, targetLabels });
  }

  get(name: string): RelationshipDef | undefined {
    return this.registry.get(name);
  }

  getAll(): RelationshipDef[] {
    return [...this.registry.values()];
  }

  getByType(type: "INTERNAL" | "PREDEFINED" | "GM_DEFINED"): RelationshipDef[] {
    return [...this.registry.values()].filter((r) => r.type === type);
  }

  isAllowedForWrite(name: string): boolean {
    const def = this.registry.get(name);
    if (!def) return false;
    return def.type === "PREDEFINED" || def.type === "GM_DEFINED";
  }

  isAllowedForRead(name: string): boolean {
    return this.registry.has(name);
  }

  // Update the description of a GM_DEFINED relationship type.
  // Returns true if updated, false if type not found or wrong category.
  updateDescription(name: string, description: string): boolean {
    const def = this.registry.get(name);
    if (!def || def.type !== "GM_DEFINED") return false;
    def.description = description;
    return true;
  }

  // Update definition fields of a GM_DEFINED relationship type.
  updateDefinition(
    name: string,
    updates: { description?: string; sourceLabels?: string[]; targetLabels?: string[] },
  ): boolean {
    const def = this.registry.get(name);
    if (!def || def.type !== "GM_DEFINED") return false;
    if (updates.description !== undefined) def.description = updates.description;
    if (updates.sourceLabels !== undefined) def.sourceLabels = updates.sourceLabels;
    if (updates.targetLabels !== undefined) def.targetLabels = updates.targetLabels;
    return true;
  }

  // Remove a GM_DEFINED relationship type from the registry.
  // Returns true if removed, false if type not found or wrong category.
  unregister(name: string): boolean {
    const def = this.registry.get(name);
    if (!def || def.type !== "GM_DEFINED") return false;
    this.registry.delete(name);
    return true;
  }

  // Clear all GM_DEFINED types from the registry (keeps INTERNAL + PREDEFINED).
  // Called on /api/reset.
  reset(): void {
    for (const [name, def] of this.registry) {
      if (def.type === "GM_DEFINED") {
        this.registry.delete(name);
      }
    }
  }

  // Sync all registered relationship types to Neo4j as :RelationshipType nodes.
  // Idempotent — safe to call multiple times.
  async syncToNeo4j(client: Neo4jClient): Promise<void> {
    for (const def of this.registry.values()) {
      await client.executeWrite(
        `MERGE (rt:RelationshipType {name: $name})
         SET rt.description = $description,
             rt.category = $category,
             rt.source_labels = $sourceLabels,
             rt.target_labels = $targetLabels`,
        {
          name: def.name,
          description: def.description,
          category: def.type,
          sourceLabels: def.sourceLabels?.length ? JSON.stringify(def.sourceLabels) : null,
          targetLabels: def.targetLabels?.length ? JSON.stringify(def.targetLabels) : null,
        },
      );
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
