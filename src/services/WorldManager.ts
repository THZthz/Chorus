import type { WorldEntity, WorldState, WorldSnapshot, Character } from "@/types/entities";
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
}

export const worldManager = new WorldManager();
