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

export interface CharacterStats {
  LOGIC: number;
  RHETORIC: number;
  EMPATHY: number;
  PERCEPTION: number;
  VOLITION: number;
  ENDURANCE: number;
  SORCERY: number;
  SUGGESTION: number;
  INSTINCT: number;
  MIGHT: number;
  CLOCKWORK: number;
  ALCHEMY: number;
}

export interface Character {
  id: string;
  type: "CHARACTER";
  displayName: string;
  shortDescription: string;
  longDescription: string;
  attributes: Record<string, string | number | boolean>;
  stats: CharacterStats;
  opinions: Record<string, string>;
  conditions: Record<string, string | number | boolean>;
}

// Type helpers for Neo4j entities (POLE+O mapping)
export type EntityType = "PERSON" | "OBJECT" | "LOCATION" | "ORGANIZATION" | "EVENT";
export type GameEntitySubtype =
  | "CHARACTER"
  | "TAVERN"
  | "TEMPLE"
  | "WORKSHOP"
  | "ARTIFACT"
  | "WEAPON"
  | "DOCUMENT"
  | "FACTION"
  | "GUILD";
