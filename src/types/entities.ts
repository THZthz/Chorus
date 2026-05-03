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
