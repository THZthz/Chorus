import type { ShortTermMemory } from "./short-term";
import type { LongTermMemory } from "./long-term";
import type { ReasoningMemory } from "./reasoning";
import type { SearchResults } from "./types";

export class MemorySearch {
  constructor(
    private shortTerm: ShortTermMemory,
    private longTerm: LongTermMemory,
    private reasoning: ReasoningMemory,
  ) {}

  async search(
    query: string,
    options?: {
      memoryTypes?: string[];
      sessionId?: string;
      limit?: number;
      threshold?: number;
    },
  ): Promise<SearchResults> {
    const { memoryTypes, sessionId, limit = 10, threshold = 0.7 } = options || {};
    const types = memoryTypes || ["messages", "entities", "preferences", "traces"];

    const results: SearchResults = {
      messages: [],
      entities: [],
      preferences: [],
      traces: [],
    };

    const tasks: Promise<void>[] = [];

    if (types.includes("messages")) {
      tasks.push(
        this.shortTerm
          .searchMessages(query, { sessionId, limit, threshold })
          .then((msgs) => {
            results.messages = msgs;
          }),
      );
    }

    if (types.includes("entities")) {
      tasks.push(
        this.longTerm
          .searchEntities(query, { limit, threshold })
          .then((entities) => {
            results.entities = entities;
          }),
      );
    }

    if (types.includes("preferences")) {
      tasks.push(
        this.longTerm.getPreferences(undefined, limit).then((prefs) => {
          results.preferences = prefs.map((p) => ({ ...p, similarity: 1.0 }));
        }),
      );
    }

    if (types.includes("traces")) {
      tasks.push(
        this.reasoning
          .getSimilarTraces(query, { limit, threshold })
          .then((traces) => {
            results.traces = traces;
          }),
      );
    }

    await Promise.all(tasks);
    return results;
  }
}
