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

import type { WorldObject, Location, Character, GameTime, SceneState } from "@/types/entities";

export interface SeedPlot {
  id: string;
  title: string;
  description: string;
  status: string;
  involvedLocations: string[];
  involvedCharacters: string[];
  childPlots: { plotId: string | null; triggerCondition: string }[];
}

export interface SeedStory {
  id: string;
  settingDescription: string;
  toneDescription: string;
  objects: Record<string, WorldObject>;
  locations: Record<string, Location>;
  characters: Record<string, Character>;
  rootPlot: SeedPlot;
  initialTime: GameTime;
  initialScene: SceneState;
}
