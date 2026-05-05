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

import type {
  WorldEntity,
  WorldState,
  WorldSnapshot,
  Character,
  GameTime,
  SceneState,
} from "@/types/entities";
import type { Plot } from "@/types/plot";

class WorldManager {
  private state: WorldState = { objects: {}, locations: {}, characters: {} };
  private plots: Plot[] = [];
  private replayOverride: WorldSnapshot | null = null;
  private listeners = new Set<() => void>();

  subscribe(fn: () => void): () => void {
    this.listeners.add(fn);
    return () => this.listeners.delete(fn);
  }

  private notify() {
    this.listeners.forEach((fn) => fn());
  }

  async loadState() {
    const [worldRes, plotsRes] = await Promise.all([fetch("/api/world"), fetch("/api/plots")]);
    if (worldRes.ok) this.state = await worldRes.json();
    if (plotsRes.ok) this.plots = await plotsRes.json();
    this.notify();
  }

  applyStepSnapshot(snapshot: WorldSnapshot | null | undefined) {
    if (!snapshot) {
      this.replayOverride = null;
      this.notify();
      return;
    }
    this.replayOverride = snapshot;
    this.notify();
  }

  clearReplayState() {
    this.replayOverride = null;
    this.notify();
  }

  isReplayActive(): boolean {
    return this.replayOverride !== null;
  }

  getState(): WorldState {
    return this.replayOverride?.entities ?? this.state;
  }

  getEntity(id: string): WorldEntity | undefined {
    const s = this.getState();
    return s.objects[id] || s.locations[id] || s.characters[id];
  }

  getAllEntities(): WorldEntity[] {
    const s = this.getState();
    return [
      ...Object.values(s.objects),
      ...Object.values(s.locations),
      ...Object.values(s.characters),
    ];
  }

  getPlots(): Plot[] {
    return this.replayOverride?.plots ?? this.plots;
  }

  getPlayerCharacter(): Character | null {
    return this.replayOverride?.playerCharacter ?? null;
  }

  updatePlotInReplaySnapshot(id: string, patch: Partial<Plot>): boolean {
    if (!this.replayOverride) return false;
    const idx = this.replayOverride.plots.findIndex((p) => p.id === id);
    if (idx === -1) return false;
    this.replayOverride.plots[idx] = { ...this.replayOverride.plots[idx], ...patch };
    this.notify();
    return true;
  }

  getReplaySnapshot(): WorldSnapshot | null {
    return this.replayOverride ?? null;
  }

  getGameTime(): GameTime | null {
    return this.replayOverride?.gameTime ?? null;
  }

  getScene(): SceneState | null {
    return this.replayOverride?.scene ?? null;
  }
}

export const worldManager = new WorldManager();
