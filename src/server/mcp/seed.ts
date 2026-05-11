import { MemoryClient } from "@/server/memory/client";
import { getActiveSeedStory } from "@/server/seed-stories/index";
import db from "@/server/db";

export async function seedDatabase(): Promise<void> {
  const story = getActiveSeedStory();
  const client = await MemoryClient.getInstance();

  console.log(`[seed] seeding ${story.entities.length} entities from "${story.id}"`);

  for (const entity of story.entities) {
    await client.longTerm.addEntity(entity.name, entity.type, {
      subtype: entity.subtype,
      description: entity.description,
      metadata: { dbId: entity.id, ...entity.metadata },
    });
  }

  for (const rel of story.relationships) {
    await client.longTerm.addRelationship(rel.sourceName, rel.targetName, rel.type, {
      description: rel.description || undefined,
    });
  }

  // Set initial time in SQLite
  db.prepare("INSERT OR REPLACE INTO system_state (key, value) VALUES (?, ?)").run(
    "game_time_day", String(story.initialDay),
  );
  db.prepare("INSERT OR REPLACE INTO system_state (key, value) VALUES (?, ?)").run(
    "game_time_segment", String(story.initialSegment),
  );

  console.log(`[seed] done — ${story.entities.length} entities, ${story.relationships.length} relationships`);
}
