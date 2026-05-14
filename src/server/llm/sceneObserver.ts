let observer: SceneObserver | null = null;

export class SceneObserver {
  private seenEntities = new Set<string>();
  private seenPlots = new Set<string>();

  wasSeen(type: "entity" | "plot", name: string): boolean {
    return type === "entity" ? this.seenEntities.has(name) : this.seenPlots.has(name);
  }

  markSeen(type: "entity" | "plot", name: string): void {
    if (type === "entity") {
      this.seenEntities.add(name);
    } else {
      this.seenPlots.add(name);
    }
  }

  reset(): void {
    this.seenEntities.clear();
    this.seenPlots.clear();
  }

  resetEntity(name: string): void {
    this.seenEntities.delete(name);
  }

  resetPlot(name: string): void {
    this.seenPlots.delete(name);
  }
}

export function getObserver(): SceneObserver {
  if (!observer) {
    observer = new SceneObserver();
  }
  return observer;
}
