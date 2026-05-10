import neo4j from "neo4j-driver";
import { v4 as uuidv4 } from "uuid";
import { getActiveSeedStory } from "@/server/seed-stories/index";
import db from "@/server/db";

function pascalCase(str: string): string {
  return str.charAt(0).toUpperCase() + str.slice(1).toLowerCase();
}

function getNeo4jDriver() {
  const uri = process.env.NEO4J_URI || "bolt://localhost:7687";
  const user = process.env.NEO4J_USER || "neo4j";
  const password = process.env.NEO4J_PASSWORD || "password";
  return neo4j.driver(uri, neo4j.auth.basic(user, password));
}

export async function seedDatabase(): Promise<void> {
  const story = getActiveSeedStory();
  const driver = getNeo4jDriver();
  const session = driver.session();

  console.log(`[seed] seeding ${story.entities.length} entities from "${story.id}"`);

  try {
    // 1. Create all entities with agent-memory compatible labels and properties
    for (const entity of story.entities) {
      const entityId = uuidv4();
      const typeLabel = pascalCase(entity.type);
      const subtypeLabel = entity.subtype ? pascalCase(entity.subtype) : null;
      const labels = subtypeLabel
        ? `:Entity:${typeLabel}:${subtypeLabel}`
        : `:Entity:${typeLabel}`;

      await session.run(
        `CREATE (e${labels} {
          id: $id,
          name: $name,
          type: $type,
          subtype: $subtype,
          description: $description,
          metadata: $metadata,
          created_at: datetime()
        })`,
        {
          id: entityId,
          name: entity.name,
          type: entity.type,
          subtype: entity.subtype || null,
          description: entity.description,
          metadata: JSON.stringify({ dbId: entity.id, ...entity.metadata }),
        },
      );
    }

    // 2. Create all relationships
    for (const rel of story.relationships) {
      await session.run(
        `MATCH (a:Entity {name: $sourceName})
         MATCH (b:Entity {name: $targetName})
         CREATE (a)-[r:${rel.type.replace(/[^A-Za-z0-9_]/g, "_")} {
           description: $description,
           created_at: datetime()
         }]->(b)`,
        {
          sourceName: rel.sourceName,
          targetName: rel.targetName,
          description: rel.description || null,
        },
      );
    }

    // 3. Set initial time in SQLite
    db.prepare("INSERT OR REPLACE INTO system_state (key, value) VALUES (?, ?)").run(
      "game_time_day", String(story.initialDay),
    );
    db.prepare("INSERT OR REPLACE INTO system_state (key, value) VALUES (?, ?)").run(
      "game_time_segment", String(story.initialSegment),
    );

    console.log(`[seed] done — ${story.entities.length} entities, ${story.relationships.length} relationships`);
  } finally {
    await session.close();
    await driver.close();
  }
}
