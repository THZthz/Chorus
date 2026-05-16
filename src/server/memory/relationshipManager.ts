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
}

const INTERNAL_TYPES: { name: string; description: string }[] = [
  {
    name: "_HAS_GM_MESSAGE",
    description: "Links a Conversation node to its GMTurnMessage nodes.",
  },
  {
    name: "_FIRST_GM_MESSAGE",
    description: "Points to the first GMTurnMessage in a Conversation's ordered linked list.",
  },
  {
    name: "_NEXT_GM_MESSAGE",
    description: "Sequentially links GMTurnMessage nodes in conversation order.",
  },
];

const PREDEFINED_TYPES: { name: string; description: string }[] = [
  {
    name: "HAS_MESSAGE",
    description: "Links a Conversation node to its Message nodes.",
  },
  {
    name: "FIRST_MESSAGE",
    description: "Points to the first Message in a Conversation's ordered linked list.",
  },
  {
    name: "NEXT_MESSAGE",
    description: "Sequentially links Message nodes in conversation order.",
  },
  {
    name: "NEXT_TIMEPOINT",
    description: "Links TimePoint nodes in chronological sequence.",
  },
  {
    name: "CURRENT_TIMEPOINT",
    description: "Points to the current TimePoint from a TimeAnchor node.",
  },
  {
    name: "AT_TIME",
    description: "Links an entity or event to a specific TimePoint.",
  },
  {
    name: "STARTED_AT",
    description: "Marks the TimePoint when an event or plot started.",
  },
  {
    name: "ACTIVE_AT",
    description: "Marks a TimePoint when an entity or condition is active.",
  },
  {
    name: "COMPLETED_AT",
    description: "Marks the TimePoint when an event or plot completed.",
  },
  {
    name: "LOCATED_AT",
    description: "An entity is physically present at a location.",
  },
  {
    name: "CARRIES",
    description: "An entity is carrying or in possession of an object.",
  },
  {
    name: "ALLIED_WITH",
    description: "An entity is allied with or friendly toward another entity.",
  },
  {
    name: "HOSTILE_TOWARDS",
    description: "An entity is hostile toward or in conflict with another entity.",
  },
  {
    name: "LOCATED_IN",
    description: "A location or entity is contained within a larger location.",
  },
  {
    name: "HAS_DISPOSITION",
    description: "Links an Entity (NPC) to its NPCDisposition nodes.",
  },
  {
    name: "ABOUT_ENTITY",
    description: "A Note is about or references an Entity.",
  },
  {
    name: "ABOUT_MESSAGE",
    description: "A Note is about or references a specific Message.",
  },
  {
    name: "BRANCHES_TO",
    description: "A parent Plot branches to a child sub-plot.",
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
    this.registry.set(name, { name, description, type });
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
      // TODO: Can we combine this into single write.
      await client.executeWrite(
        `MERGE (rt:RelationshipType {name: $name})
         SET rt.description = $description, rt.category = $category`,
        { name: def.name, description: def.description, category: def.type },
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
