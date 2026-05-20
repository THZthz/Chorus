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

export type EntityType = "CHARACTER" | "OBJECT" | "LOCATION";

export interface MemoryEntity {
  name: string;
  type: EntityType;
  subtype?: string;
  description?: string;
  brief?: string;
  aliases: string[];
  metadata: Record<string, unknown>;
  _embedding?: number[];
  // True if the entity was newly created (MERGE semantics).
  isNew?: boolean;
}

export interface MemoryMessage {
  id: string;
  content: string;
  metadata: Record<string, unknown>;
  _embedding?: number[];
}

export interface MemoryNote {
  name: string;
  content: string;
  _embedding?: number[];
}

export const PLOT_STATUSES = [
  "PENDING",
  "ACTIVE",
  "IN_PROGRESS",
  "COMPLETED",
  "ABANDONED",
] as const;
export type PlotStatus = (typeof PLOT_STATUSES)[number];

export interface PlotFlag {
  flagId: string;
  description: string;
}

export interface MemoryPlot {
  name: string;
  description: string;
  brief?: string;
  status: PlotStatus;
  triggerCondition?: string;
  flags: PlotFlag[];
  _embedding?: number[];
}

export interface Disposition {
  npcName: string;
  targetName: string;
  sentiment: string; // TODO: Delete this useless property.
  summary: string;
}

export interface PlayerCondition {
  description: string;
  effects: Array<{ stat?: string; modifier: number; description?: string }>;
  duration?: string;
  source?: string;
}

export interface SearchResults {
  messages: Array<MemoryMessage & { similarity: number; relevance?: number }>;
  entities: Array<MemoryEntity & { similarity: number; relevance?: number }>;
}
