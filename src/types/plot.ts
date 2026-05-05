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

export interface PlotOption {
  plotId: string | null;
  triggerCondition: string;
}

export const PLOT_STATUSES = ["PENDING", "IN_PROGRESS", "RESOLVED"] as const;
export type PlotStatus = (typeof PLOT_STATUSES)[number];

export interface Plot {
  id: string;
  title: string;
  description: string;
  status: PlotStatus;
  involvedLocations: string[];
  involvedCharacters: string[];
  parentPlotId: string | null;
  parentOptionId: number | null;
  childPlots: PlotOption[];
}

export type PlotPatch = Partial<
  Pick<
    Plot,
    "title" | "status" | "description" | "involvedLocations" | "involvedCharacters" | "childPlots"
  >
>;
