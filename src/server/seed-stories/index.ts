/**
 * Chorus — cinematic dialogue engine
 * Copyright (C) 2026 Amias 1289941679@qq.com
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
  "glass-cage": loadSeedStory("glass-cage.toml"),
};

const ACTIVE_SEED_STORY = "glass-cage";

export function getActiveSeedStory(): SeedStory {
  const story = STORIES[ACTIVE_SEED_STORY];
  if (!story) throw new Error(`Seed story "${ACTIVE_SEED_STORY}" not found`);
  return story;
}
