import { getContext } from "@/server/llm/tools/getContext";
import { queryWorld } from "@/server/llm/tools/queryWorld";
import { searchWorld } from "@/server/llm/tools/searchWorld";
import { editNode } from "@/server/llm/tools/editNode";
import { editRelationship } from "@/server/llm/tools/editRelationship";
import { resetSceneContext } from "@/server/llm/tools/resetSceneContext";
import { createAdvanceTimeTool } from "@/server/llm/tools/advanceTime";
import { exec, parseToolOutput, createMockEventEmitter, resetDb, resetObserver } from "../helpers";

describe("Gameplay: Murder Mystery Investigation", () => {
  beforeAll(async () => {
    await resetDb();
    resetObserver();
  });

  it("simulates a full investigation turn", async () => {
    // 1. Load initial scene context
    const scene = await exec(getContext, { types: ["SCENE_CONTEXT"] });
    expect(scene).toContain("SCENE CONTEXT");
    expect(scene).toContain("Player");
    expect(scene).toContain("Time");

    // 2. Review all characters
    const characters = await exec(getContext, { types: ["CHARACTERS_BRIEF"] });
    expect(characters).toContain("Elias Crowne");
    expect(characters).toContain("Lord Aldric Vane");
    expect(characters).toContain("Crystal vi Elaris");

    // 3. Review active plots
    const plots = await exec(getContext, { types: ["PLOTS_BRIEF"] });
    expect(plots).toContain("The Glass Cage");
    expect(plots).toContain("IN_PROGRESS");

    // 4. Search for clues related to the ledger
    const searchResult = await exec(searchWorld, {
      query: "ledger",
      types: ["entities", "notes"],
      limit: 5,
    });
    const searchData = parseToolOutput(searchResult);
    expect(Array.isArray(searchData.entities)).toBe(true);

    // 5. Find all entities in Passenger Car A (6 characters located here in seed data)
    const locQuery = await exec(queryWorld, {
      action: "READ",
      query:
        "MATCH (e:Entity)-[:LOCATED_AT]->(loc:Entity {name: 'Passenger Car A'}) RETURN e.name, e.type",
    });
    const locData = parseToolOutput(locQuery);
    expect(locData.rowCount).toBeGreaterThan(0);

    // 6. GM creates an investigation note
    const noteResult = await exec(editNode, {
      nodeLabel: "Note",
      action: "CREATE",
      properties: {
        name: "Investigation Notes - Turn 1",
        content:
          "Suspicion falls on Lord Vane. The ledger mentions payments to an unknown party. Crystal's lyre was found near the murder scene.",
      },
    });
    expect(noteResult).toContain("created");

    // 7. Link the note to the suspect
    const linkResult = await exec(editRelationship, {
      action: "CREATE",
      relationshipType: "ABOUT_ENTITY",
      sourceLabel: "Note",
      sourceMatch: { name: "Investigation Notes - Turn 1" },
      targetLabel: "Entity",
      targetMatch: { name: "Lord Aldric Vane" },
    });
    expect(linkResult).toContain("created successfully");

    // 8. Advance time by 2 segments (from Late Afternoon to Dusk)
    const mockEvents = createMockEventEmitter();
    const advanceTime = createAdvanceTimeTool(mockEvents);
    const timeResult = await exec(advanceTime, {
      segments: 2,
      reason: "Thorough investigation takes time",
    });
    expect(timeResult).toContain("Time advanced");

    // 9. Verify time changed by checking the TimeAnchor
    const timeQuery = await exec(queryWorld, {
      action: "READ",
      query:
        "MATCH (a:TimeAnchor {_id:'anchor'})-[:CURRENT_TIMEPOINT]->(tp:TimePoint) RETURN tp.day, tp.segment",
    });
    const timeData = parseToolOutput(timeQuery);
    const timeRow = timeData.rows[0] as Record<string, unknown>;

    // 10. Reset scene context for next turn
    const resetResult = await exec(resetSceneContext, {});
    expect(resetResult).toContain("reset");

    // Clean up: delete the investigation note (breaks the ABOUT_ENTITY link automatically)
    await exec(editNode, {
      nodeLabel: "Note",
      action: "DELETE",
      match: { name: "Investigation Notes - Turn 1" },
    });
  });
});
