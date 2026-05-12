import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { parse } from "smol-toml";
import type { SeedStory } from "@/server/seed-stories/types";

const __dirname = dirname(fileURLToPath(import.meta.url));

function loadSeedStory(filename: string): SeedStory {
  const filePath = join(__dirname, filename);
  const toml = readFileSync(filePath, "utf-8");
  return parse(toml) as unknown as SeedStory;
}

const STORIES: Record<string, SeedStory> = {
  "magic-awakening": loadSeedStory("magic-awakening.toml"),
};

const ACTIVE_SEED_STORY = "magic-awakening";

export function getActiveSeedStory(): SeedStory {
  const story = STORIES[ACTIVE_SEED_STORY];
  if (!story) throw new Error(`Seed story "${ACTIVE_SEED_STORY}" not found`);
  return story;
}
