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

  console.log(
    `[seed] done — ${story.entities.length} entities, ${story.relationships.length} relationships`,
  );
}
