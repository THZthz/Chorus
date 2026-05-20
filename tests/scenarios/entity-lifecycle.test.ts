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

import { editNode } from "@/server/llm/tools/editNode";
import { editRelationship } from "@/server/llm/tools/editRelationship";
import { queryWorld } from "@/server/llm/tools/queryWorld";
import { manageSchema } from "@/server/llm/tools/manageSchema";
import { getContext } from "@/server/llm/tools/getContext";
import { exec, parseToolOutput, resetDb } from "../helpers";

describe("Entity Lifecycle Scenario", () => {
  beforeAll(async () => {
    await resetDb();
  });

  const CLUE_TYPE = "TestClue";
  const CLUE_NAME = "Bloody Dagger";

  it("full lifecycle: register type -> create -> update -> relate -> query -> delete -> unregister", async () => {
    // 1. Register a new node type
    const regResult = await exec(manageSchema, {
      target: "NODE",
      action: "REGISTER",
      name: CLUE_TYPE,
      description: "An investigation clue",
      properties: [
        { name: "name", description: "The clue's name", tags: ["string"] },
        { name: "found_by", description: "Who found it", tags: ["string"] },
        { name: "location_found", description: "Where it was found", tags: ["string"] },
      ],
    });
    expect(regResult).toContain("Registered node type");
    expect(regResult).toContain(CLUE_TYPE);

    // 2. Create the clue entity
    const createResult = await exec(editNode, {
      nodeLabel: CLUE_TYPE,
      action: "CREATE",
      properties: {
        name: CLUE_NAME,
        found_by: "Player",
        location_found: "Passenger Car A",
      },
    });
    expect(createResult).toContain("created");

    // 3. Query to verify it exists
    const read1 = await exec(queryWorld, {
      action: "READ",
      query: `MATCH (c:${CLUE_TYPE} {name: '${CLUE_NAME}'}) RETURN c.name, c.found_by`,
    });
    const data1 = parseToolOutput(read1);
    expect(data1.rowCount).toBe(1);

    // 4. Update the clue with new findings
    const updateResult = await exec(editNode, {
      nodeLabel: CLUE_TYPE,
      action: "UPDATE",
      match: { name: CLUE_NAME },
      properties: { found_by: "Elias Crowne", location_found: "Engine Car" },
    });
    expect(updateResult).toContain("updated");

    // 5. Verify update
    const read2 = await exec(queryWorld, {
      action: "READ",
      query: `MATCH (c:${CLUE_TYPE} {name: '${CLUE_NAME}'}) RETURN c.found_by, c.location_found`,
    });
    const data2 = parseToolOutput(read2);
    const row = data2.rows[0] as Record<string, unknown>;
    expect(row["c.found_by"]).toBe("Elias Crowne");
    expect(row["c.location_found"]).toBe("Engine Car");

    // 6. Register a relationship type for TestClue→Entity
    await exec(manageSchema, {
      target: "RELATIONSHIP",
      action: "REGISTER",
      name: "LOCATED_AT",
      description: "A test clue is located at an entity",
      sourceLabel: CLUE_TYPE,
      targetLabel: "Entity",
    });

    // 7. Create a relationship: locate clue at Engine Car
    const relResult = await exec(editRelationship, {
      action: "CREATE",
      relationshipType: "LOCATED_AT",
      sourceLabel: CLUE_TYPE,
      sourceMatch: { name: CLUE_NAME },
      targetLabel: "Entity",
      targetMatch: { name: "Engine Car" },
    });
    expect(relResult).toContain("created successfully");

    // 8. Verify relationship exists
    const read3 = await exec(queryWorld, {
      action: "READ",
      query: `MATCH (c:${CLUE_TYPE} {name: '${CLUE_NAME}'})-[r:LOCATED_AT]->(e:Entity) RETURN e.name`,
    });
    const data3 = parseToolOutput(read3);
    expect(data3.rowCount).toBe(1);

    // 9. Delete the clue
    const deleteResult = await exec(editNode, {
      nodeLabel: CLUE_TYPE,
      action: "DELETE",
      match: { name: CLUE_NAME },
    });
    expect(deleteResult).toContain("deleted");

    // 10. Verify deletion
    const read4 = await exec(queryWorld, {
      action: "READ",
      query: `MATCH (c:${CLUE_TYPE} {name: '${CLUE_NAME}'}) RETURN c`,
    });
    const data4 = parseToolOutput(read4);
    expect(data4.rowCount).toBe(0);

    // 11. Unregister the relationship type
    await exec(manageSchema, {
      target: "RELATIONSHIP",
      action: "UNREGISTER",
      name: "LOCATED_AT",
      sourceLabel: CLUE_TYPE,
      targetLabel: "Entity",
    });

    // 12. Unregister the node type
    const unregResult = await exec(manageSchema, {
      target: "NODE",
      action: "UNREGISTER",
      name: CLUE_TYPE,
    });
    expect(unregResult).toContain("Unregistered node type");
  });
});
