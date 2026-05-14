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
import { migrateToTimePoints } from "@/server/models/time";

function inferSentiment(text: string): string {
  const lower = text.toLowerCase();
  if (lower.includes("protect") || lower.includes("care") || lower.includes("save"))
    return "protective";
  if (lower.includes("trust") || lower.includes("believe")) return "trusting";
  if (lower.includes("fear") || lower.includes("dangerous") || lower.includes("risk"))
    return "fearful";
  if (lower.includes("hate") || lower.includes("hostile") || lower.includes("kill"))
    return "hostile";
  if (
    lower.includes("attract") ||
    lower.includes("desire") ||
    lower.includes("hunger") ||
    lower.includes("want")
  )
    return "attracted";
  if (lower.includes("suspicious") || lower.includes("suspect") || lower.includes("hiding"))
    return "suspicious";
  if (lower.includes("resent") || lower.includes("bitter") || lower.includes("angry"))
    return "resentful";
  if (lower.includes("grateful") || lower.includes("thank") || lower.includes("owe"))
    return "grateful";
  return "indifferent";
}

export async function seedDatabase(): Promise<void> {
  const story = getActiveSeedStory();
  const client = await MemoryClient.getInstance();

  console.log(`[seed] seeding ${story.entities.length} entities from "${story.id}"`);

  for (const entity of story.entities) {
    await client.longTerm.addEntity(entity.name, entity.type, {
      subtype: entity.subtype,
      description: entity.description,
      metadata: entity.metadata,
    });
  }

  for (const rel of story.relationships) {
    await client.longTerm.addRelationship(rel.sourceName, rel.targetName, rel.type, {
      description: rel.description || undefined,
    });
  }

  // Seed initial NPC dispositions from entity metadata opinions
  let dispositionCount = 0;
  for (const entity of story.entities) {
    const opinions = entity.metadata?.opinions as Record<string, string> | undefined;
    if (!opinions) continue;
    for (const [targetName, opinionText] of Object.entries(opinions)) {
      const sentiment = inferSentiment(opinionText);
      await client.longTerm.setDisposition(entity.name, targetName, sentiment, opinionText);
      dispositionCount++;
    }
  }

  // Seed plots from story
  for (const plot of story.plots || []) {
    await client.plots.createPlot(plot.name, {
      description: plot.description,
      status: plot.status,
      triggerCondition: plot.triggerCondition,
      flags: plot.flags,
    });
  }

  // Seed plot branches
  for (const plot of story.plots || []) {
    if (plot.branchesTo) {
      for (const childName of plot.branchesTo) {
        await client.plots.branchTo(plot.name, childName);
      }
    }
  }

  // Initialize TimePoint system (idempotent migration)
  await migrateToTimePoints(story.initialDay, story.initialSegment);

  console.log(
    `[seed] done — ${story.entities.length} entities, ${story.relationships.length} relationships, ${dispositionCount} dispositions`,
  );
}
