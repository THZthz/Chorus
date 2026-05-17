import { tool } from "ai";
import { z } from "zod";
import { TOOL_NAMES } from "@/shared/constants";
import { wrapSafe } from "@/server/llm/tools/shared";
import {
  buildSceneContext,
  buildCharactersBrief,
  buildLocationsBrief,
  buildObjectsBrief,
  buildPlotsBrief,
  buildRelationshipDump,
} from "@/server/llm/sceneContext";
import {
  getSchemaVisualization,
  getRelationshipTypeDescriptions,
  formatSchemaMarkdown,
} from "@/server/models/schema";
import { MemoryClient } from "@/server/memory/client";

const CONTEXT_TYPES = [
  "SCENE_CONTEXT",
  "CHARACTERS_BRIEF",
  "LOCATIONS_BRIEF",
  "OBJECTS_BRIEF",
  "PLOTS_BRIEF",
  "SCHEMA_DUMP",
  "RELATIONSHIP_DUMP",
] as const;

type ContextType = (typeof CONTEXT_TYPES)[number];

async function buildSchemaDump(): Promise<string> {
  const db = MemoryClient.getCachedInstance().neo4j;
  const [schemaVis, relTypeDescs] = await Promise.all([
    getSchemaVisualization(db).catch((err) => {
      console.error("[getContext] schema visualization failed:", err);
      return { nodes: [], relationships: [] };
    }),
    getRelationshipTypeDescriptions(db).catch((err) => {
      console.error("[getContext] relationship type descriptions failed:", err);
      return [] as { name: string; description: string; category: string }[];
    }),
  ]);
  return formatSchemaMarkdown(schemaVis, relTypeDescs);
}

export const getContext = tool({
  title: TOOL_NAMES.GET_CONTEXT,
  description: `
Pull context from the world archive on demand. Nothing is auto-loaded — you decide what you need.

Types:
- SCENE_CONTEXT — Your immediate surroundings: time, location, nearby NPCs/objects,
  inventory, NPC dispositions, active plots. Shows full descriptions for unseen entities/plots;
  compact briefs thereafter.
- CHARACTERS_BRIEF — All characters with current location and disposition toward player.
- LOCATIONS_BRIEF — All locations with brief descriptions.
- OBJECTS_BRIEF — All objects with who carries them or where they are.
- PLOTS_BRIEF — All plots with status, brief, and flags.
- SCHEMA_DUMP — Available node types (with property schemas) and relationship types
  (with endpoint constraints).
- RELATIONSHIP_DUMP — All non-internal relationships, grouped by type. LOCATED_AT and
  LOCATED_IN are grouped by location showing occupants.

Default (no types specified): SCENE_CONTEXT only.
Call with additional types when you need global awareness beyond your immediate scene.
`.trim(),
  inputSchema: z.object({
    types: z
      .array(z.enum(CONTEXT_TYPES))
      .default(["SCENE_CONTEXT"])
      .describe("Which context sections to return. Default: SCENE_CONTEXT only."),
  }),
  execute: wrapSafe(async (args: { types: ContextType[] }) => {
    const sections = args.types.length > 0 ? args.types : ["SCENE_CONTEXT"];

    const builders: Record<ContextType, () => Promise<string>> = {
      SCENE_CONTEXT: buildSceneContext,
      CHARACTERS_BRIEF: buildCharactersBrief,
      LOCATIONS_BRIEF: buildLocationsBrief,
      OBJECTS_BRIEF: buildObjectsBrief,
      PLOTS_BRIEF: buildPlotsBrief,
      SCHEMA_DUMP: buildSchemaDump,
      RELATIONSHIP_DUMP: buildRelationshipDump,
    };

    const results: string[] = [];
    for (const type of sections) {
      try {
        const section = await builders[type]();
        if (section) results.push(section);
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        results.push(`## ${type}\n\nError: ${msg}\n`);
      }
    }

    return results.join("\n");
  }, TOOL_NAMES.GET_CONTEXT),
});
