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
import {NOTIFICATION_TYPES} from "@/types/dialogue.ts";

// Entity types following POLE+O model
export type EntityType = "PERSON" | "OBJECT" | "LOCATION" | "ORGANIZATION" | "EVENT";

export type MessageRole = "user" | "assistant" | "system";

export interface MemoryEntity {
  id: string;
  name: string;
  type: EntityType;
  subtype?: string;
  description?: string;
  aliases: string[];
  metadata: Record<string, unknown>;
  _embedding?: number[];
  createdAt: Date;
  /** True if the entity was newly created (MERGE semantics). */
  isNew?: boolean;
}

export interface MemoryMessage {
  id: string;
  role: MessageRole;
  content: string;
  metadata: Record<string, unknown>;
  _embedding?: number[];
  createdAt: Date;
}

export interface EntityRelationship {
  id: string;
  sourceId: string;
  targetId: string;
  type: string;
  description?: string;
  confidence: number;
}

export interface MemoryNote {
  id: string;
  content: string;
  _embedding?: number[];
  createdAt: Date;
  updatedAt: Date;
}

export const PLOT_STATUSES = ["PENDING", "ACTIVE", "IN_PROGRESS", "COMPLETED", "ABANDONED"] as const;
export type PlotStatus = (typeof PLOT_STATUSES)[number];

export interface PlotFlag {
  flagId: string;
  description: string;
}

export interface MemoryPlot {
  id: string;
  name: string;
  description: string;
  status: PlotStatus;
  triggerCondition?: string;
  flags: PlotFlag[];
  _embedding?: number[];
  createdAt: Date;
  updatedAt: Date;
}

export interface NPCDisposition {
  id: string;
  npcName: string;
  targetName: string;
  sentiment: string;
  summary: string;
  createdAt: Date;
  updatedAt: Date;
}

export interface PlayerCondition {
  description: string;
  effects: Array<{ stat?: string; modifier: number; description?: string }>;
  duration?: string;
  source?: string;
}

export interface SearchResults {
  messages: Array<MemoryMessage & { similarity: number }>;
  entities: Array<MemoryEntity & { similarity: number }>;
}
