// NOTE: sourceName/targetName must be entity names (the `name` field stored in Neo4j),
// NOT database IDs. agent-memory's memory_create_relationship looks up entities by name.
export interface SeedEntity {
  id: string;
  type: "PERSON" | "OBJECT" | "LOCATION" | "ORGANIZATION" | "EVENT";
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

export interface SeedStory {
  id: string;
  settingDescription: string;
  toneDescription: string;
  entities: SeedEntity[];
  relationships: SeedRelationship[];
  initialDay: number;
  initialSegment: number;
  initialLocationId: string;
}
