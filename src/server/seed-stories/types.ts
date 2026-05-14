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

// NOTE: sourceName/targetName must be entity names (the `name` field stored in Neo4j),
// NOT database IDs. agent-memory's memory_create_relationship looks up entities by name.
export interface SeedEntity {
  id: string;
  type: "CHARACTER" | "OBJECT" | "LOCATION" | "ORGANIZATION" | "EVENT";
  subtype?: string;
  name: string;
  description: string;
  metadata?: Record<string, unknown>;
}

export interface SeedRelationship {
  sourceName: string;
  targetName: string;
  type: string; // e.g., "LOCATED_AT", "CARRIES", "HOSTILE_TOWARDS"
  description?: string;
}

export interface SeedPlot {
  name: string;
  description: string;
  status: "PENDING" | "ACTIVE" | "IN_PROGRESS" | "COMPLETED" | "ABANDONED";
  triggerCondition?: string;
  flags?: Array<{ flagId: string; description: string }>;
  branchesTo?: string[];
}

export interface SeedStory {
  id: string;
  settingDescription: string;
  toneDescription: string;
  entities: SeedEntity[];
  relationships: SeedRelationship[];
  plots?: SeedPlot[];
  initialDay: number;
  initialSegment: number;
  initialLocationId: string;
}
