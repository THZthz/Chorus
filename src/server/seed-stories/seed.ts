import { MemoryClient } from "@/server/memory/client";
import { getActiveSeedStory } from "@/server/seed-stories";
import { setGameTime } from "@/server/models/time";

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
      metadata: { dbId: entity.id, ...entity.metadata },
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

  // Seed initial player flags
  await client.neo4j.executeWrite(`MATCH (e:Entity {name: "Player"}) SET e:PlayerCharacter`);
  await client.longTerm.setPlayerFlag(
    "has_soul_shard",
    "Carries a violet crystal that pulses with the player's heartbeat — responds to emotion and magic",
    "Woke holding it in the rain outside Sinking Dock",
  );
  await client.longTerm.setPlayerFlag(
    "has_veyllas_ribbon",
    "Wears Veyla's midnight-blue silk ribbon tied around the wrist — a protective charm",
    "Veyla tied it while the player slept",
  );

  // Set initial time in Neo4j
  await setGameTime({ day: story.initialDay, segment: story.initialSegment });

  console.log(
    `[seed] done — ${story.entities.length} entities, ${story.relationships.length} relationships, ${dispositionCount} dispositions`,
  );
}
