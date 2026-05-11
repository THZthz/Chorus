import { MemoryClient } from "@/server/memory/client";
import { getActiveSeedStory } from "@/server/seed-stories";
import { setGameTime } from "@/server/models/time";

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

  // Set initial time in Neo4j
  await setGameTime({ day: story.initialDay, segment: story.initialSegment });

  console.log(`[seed] done — ${story.entities.length} entities, ${story.relationships.length} relationships`);
}
