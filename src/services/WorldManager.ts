import { WorldEntity, WorldState, Character, Location, WorldObject } from "@/types/entities";

class WorldManager {
  private state: WorldState = {
    objects: {},
    locations: {},
    characters: {},
  };

  async loadState() {
    const res = await fetch("/api/world");
    if (res.ok) {
      this.state = await res.json();
    }
  }

  getEntity(id: string): WorldEntity | undefined {
    return this.state.objects[id] || this.state.locations[id] || this.state.characters[id];
  }

  getState(): WorldState {
    return this.state;
  }

  getAllEntities(): WorldEntity[] {
    return [
      ...Object.values(this.state.objects),
      ...Object.values(this.state.locations),
      ...Object.values(this.state.characters),
    ];
  }
}

export const worldManager = new WorldManager();
