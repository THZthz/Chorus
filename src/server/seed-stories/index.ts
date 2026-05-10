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

import type { SeedStory } from "./types";
import { magicAwakening } from "./magic-awakening";
import { ironSerpentMurder } from "./iron-serpent-murder";
import { celestialAthenaeum } from "./celestial-athenaeum";

export const ACTIVE_SEED_STORY = "magic-awakening";

const STORIES: Record<string, SeedStory> = {
  "magic-awakening": magicAwakening,
  "iron-serpent-murder": ironSerpentMurder,
  "celestial-athenaeum": celestialAthenaeum,
};

export function getActiveSeedStory(): SeedStory {
  const story = STORIES[ACTIVE_SEED_STORY];
  if (!story) {
    throw new Error(
      `Unknown seed story: "${ACTIVE_SEED_STORY}". Available: ${Object.keys(STORIES).join(", ")}`,
    );
  }
  return story;
}
