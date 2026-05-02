import { WorldEntity, WorldState, Character, Location, WorldObject } from "@/types/entities";
import type { Plot } from "@/types/plot";

class WorldManager {
  private state: WorldState = { objects: {}, locations: {}, characters: {} };
  private plots: Plot[] = [];
  private replayOverride: {
    entities: WorldState;
    plots: Plot[];
    playerCharacter: Character | null;
  } | null = null;
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

  applyStepSnapshot(snapshot: Record<string, unknown> | null | undefined) {
    if (!snapshot) {
      this.replayOverride = null;
      this.notify();
      return;
    }
    this.replayOverride = {
      entities: snapshot.entities as WorldState,
      plots: (snapshot.plots as Plot[]) ?? [],
      playerCharacter: (snapshot.playerCharacter as Character) ?? null,
    };
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
}

export const worldManager = new WorldManager();
