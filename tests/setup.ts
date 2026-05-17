import "dotenv/config";
import { MemoryClient } from "@/server/memory/client";
import { seedDatabase } from "@/server/seed-stories/seed";
import { getObserver } from "@/server/llm/sceneObserver";

export async function setup() {
  await MemoryClient.getInstance();
  await seedDatabase();
  getObserver().reset();
}
