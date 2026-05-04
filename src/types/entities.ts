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

import type { Plot } from "@/types/plot";

export type EntityType = "OBJECT" | "LOCATION" | "CHARACTER";

export interface CharacterStats {
  logic: number;
  rhetoric: number;
  empathy: number;
  perception: number;
  volition: number;
  endurance: number;
  sorcery: number;
  suggestion: number;
  instinct: number;
  might: number;
  clockwork: number;
  alchemy: number;
}

export interface BaseEntity {
  id: string;
  type: EntityType;
  displayName: string;
  shortDescription: string;
  longDescription: string;
  attributes: Record<string, string | number | boolean>;
}

export interface WorldObject extends BaseEntity {
  type: "OBJECT";
}

export interface Location extends BaseEntity {
  type: "LOCATION";
}

export interface Character extends BaseEntity {
  type: "CHARACTER";
  stats: Record<string, number>;
  opinions: Record<string, string>; // characterId -> opinion text
}

export type WorldEntity = WorldObject | Location | Character;

export interface WorldState {
  objects: Record<string, WorldObject>;
  locations: Record<string, Location>;
  characters: Record<string, Character>;
}

export interface WorldSnapshot {
  entities: WorldState;
  plots: Plot[];
  playerCharacter: Character | null;
}
