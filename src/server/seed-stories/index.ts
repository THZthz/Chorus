import type { SeedStory } from "@/server/seed-stories/types";
import { magicAwakening } from "@/server/seed-stories/magic-awakening";

const STORIES: Record<string, SeedStory> = {
  "magic-awakening": magicAwakening,
};

const ACTIVE_SEED_STORY = "magic-awakening";

export function getActiveSeedStory(): SeedStory {
  const story = STORIES[ACTIVE_SEED_STORY];
  if (!story) throw new Error(`Seed story "${ACTIVE_SEED_STORY}" not found`);
  return story;
}
