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
    console.trace(`[resetEntity] reset entity: ${name}`);
    this.seenEntities.delete(name);
  }

  resetPlot(name: string): void {
    console.trace(`[resetPlot] reset plot: ${name}`);
    this.seenPlots.delete(name);
  }
}

export function getObserver(): SceneObserver {
  if (!observer) {
    observer = new SceneObserver();
  }
  return observer;
}

/**
 * Check a Cypher write query for entity description/brief changes and reset the
 * observer for affected entities so the GM sees the new description next turn.
 */
export function resetEntityForQuery(query: string): void {
  const obs = getObserver();

  // Match patterns like: MATCH (e:Entity {name: "Veyla"}) ... SET e.description = ...
  const nameRegex = /\bMATCH\s*\([^)]*:Entity\s*\{[^}]*name:\s*"([^"]+)"\s*\}/gi;
  const names: string[] = [];
  let match: string[];
  while ((match = nameRegex.exec(query)) !== null) {
    names.push(match[1]);
  }

  // Also match: MATCH (e:Entity) WHERE e.name = "X" ...
  const whereRegex = /MATCH\s*\([^)]*:Entity[^)]*\)\s*(?:WHERE\s+\w+\.name\s*=\s*"([^"]+)")/gi;
  while ((match = whereRegex.exec(query)) !== null) {
    names.push(match[1]);
  }

  if (names.length === 0) return;

  const descChanged = /SET\s+\w+\.description\s*=/i.test(query);
  const briefChanged = /SET\s+\w+\.brief\s*=/i.test(query);

  if (descChanged || briefChanged) {
    for (const name of names) {
      obs.resetEntity(name);
    }
  }
}
