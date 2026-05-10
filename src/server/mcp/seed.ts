import { getMcpClient } from "@/server/mcp/client";
import { getActiveSeedStory } from "@/server/seed-stories/index";
import db from "@/server/db";

export async function seedDatabase(): Promise<void> {
  const story = getActiveSeedStory();
  const client = await getMcpClient();
  const tools = await client.tools();

  console.log(`[seed] seeding ${story.entities.length} entities from "${story.id}"`);

  // 1. Create all entities
  for (const entity of story.entities) {
    const result = await tools.memory_add_entity.execute({
      name: entity.name,
      entity_type: entity.type,
      subtype: entity.subtype,
      description: entity.description,
      metadata: {
        id: entity.id,
        ...entity.metadata,
      },
    }, { messages: [], toolCallId: `seed-entity-${entity.id}` });

    const parsed = typeof result === "string" ? JSON.parse(result) : result;
    if (parsed && typeof parsed === "object" && "error" in parsed) {
      console.error(`[seed] failed to create entity ${entity.name}: ${parsed.error}`);
    }
  }

  // 2. Create all relationships
  for (const rel of story.relationships) {
    const result = await tools.memory_create_relationship.execute({
      source_name: rel.sourceName,
      target_name: rel.targetName,
      relationship_type: rel.type,
      description: rel.description,
    }, { messages: [], toolCallId: `seed-rel-${rel.sourceName}-${rel.targetName}` });

    const parsed = typeof result === "string" ? JSON.parse(result) : result;
    if (parsed && typeof parsed === "object" && "error" in parsed) {
      console.error(`[seed] failed to create relationship ${rel.type}: ${parsed.error}`);
    }
  }

  // 3. Set initial time in SQLite
  db.prepare("INSERT OR REPLACE INTO system_state (key, value) VALUES (?, ?)").run(
    "game_time_day", String(story.initialDay),
  );
  db.prepare("INSERT OR REPLACE INTO system_state (key, value) VALUES (?, ?)").run(
    "game_time_segment", String(story.initialSegment),
  );

  console.log(`[seed] done — ${story.entities.length} entities, ${story.relationships.length} relationships`);
}
