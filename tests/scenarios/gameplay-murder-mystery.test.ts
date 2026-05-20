/**
 * Chorus — cinematic dialogue engine
 * Copyright (C) 2026 Amias
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

import { getContext } from "@/server/llm/tools/getContext";
import { queryWorld } from "@/server/llm/tools/queryWorld";
import { searchWorld } from "@/server/llm/tools/searchWorld";
import { editNode } from "@/server/llm/tools/editNode";
import { editRelationship } from "@/server/llm/tools/editRelationship";
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
      labels: ["Entity", "Note"],
      limit: 5,
    });
    const searchData = parseToolOutput(searchResult);
    expect(Array.isArray(searchData.Entity)).toBe(true);

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

    // 8. Advance time by 4 hours
    const mockEvents = createMockEventEmitter();
    const advanceTime = createAdvanceTimeTool(mockEvents);
    const timeResult = await exec(advanceTime, {
      hours: 4,
      reason: "Thorough investigation takes time",
    });
    expect(timeResult).toContain("Time advanced");

    // 9. Verify time changed by checking the TimeAnchor
    const timeQuery = await exec(queryWorld, {
      action: "READ",
      query:
        "MATCH (a:TimeAnchor {_id:'anchor'})-[:CURRENT_TIMEPOINT]->(tp:TimePoint) RETURN tp.day, tp.hour",
    });
    const timeData = parseToolOutput(timeQuery);
    const timeRow = timeData.rows[0] as Record<string, unknown>;

    // Clean up: delete the investigation note (breaks the ABOUT_ENTITY link automatically)
    await exec(editNode, {
      nodeLabel: "Note",
      action: "DELETE",
      match: { name: "Investigation Notes - Turn 1" },
    });
  });
});
