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
import { exec, resetDb } from "../helpers";

describe("getContext", () => {
  beforeAll(async () => {
    await resetDb();
  });

  it("returns SCENE_CONTEXT by default", async () => {
    const result = await exec(getContext, { types: ["SCENE_CONTEXT"] });
    expect(result).toContain("SCENE CONTEXT");
    expect(result).toContain("Player");
    expect(result).toContain("Time");
  });

  it("returns CHARACTERS_BRIEF with all characters", async () => {
    const result = await exec(getContext, { types: ["CHARACTERS_BRIEF"] });
    expect(result).toContain("## CHARACTERS");
    expect(result).toContain("Elias Crowne");
    expect(result).toContain("Lord Aldric Vane");
    expect(result).toContain("Player");
  });

  it("returns LOCATIONS_BRIEF", async () => {
    const result = await exec(getContext, { types: ["LOCATIONS_BRIEF"] });
    expect(result).toContain("## LOCATIONS");
    expect(result).toContain("Engine Car");
    expect(result).toContain("Dining Car");
  });

  it("returns OBJECTS_BRIEF", async () => {
    const result = await exec(getContext, { types: ["OBJECTS_BRIEF"] });
    expect(result).toContain("## OBJECTS");
    expect(result).toContain("Crowne's Ledger");
    expect(result).toContain("Player's Key");
  });

  it("returns PLOTS_BRIEF", async () => {
    const result = await exec(getContext, { types: ["PLOTS_BRIEF"] });
    expect(result).toContain("## PLOTS");
    expect(result).toContain("The Glass Cage");
    expect(result).toContain("The Hunter");
  });

  it("returns SCHEMA_DUMP", async () => {
    const result = await exec(getContext, { types: ["SCHEMA_DUMP"] });
    expect(result).toContain("## Schema");
    expect(result).toContain("Entity");
    expect(result).toContain("LOCATED_AT");
  });

  it("returns RELATIONSHIP_DUMP", async () => {
    const result = await exec(getContext, { types: ["RELATIONSHIP_DUMP"] });
    expect(result).toContain("## RELATIONSHIPS");
  });

  it("returns multiple types at once", async () => {
    const result = await exec(getContext, {
      types: ["SCENE_CONTEXT", "CHARACTERS_BRIEF", "LOCATIONS_BRIEF", "PLOTS_BRIEF"],
    });
    expect(result).toContain("SCENE CONTEXT");
    expect(result).toContain("## CHARACTERS");
    expect(result).toContain("## LOCATIONS");
    expect(result).toContain("## PLOTS");
  });

  it("defaults to SCENE_CONTEXT when types array is empty", async () => {
    const result = await exec(getContext, { types: [] });
    expect(result).toContain("SCENE CONTEXT");
  });

  it("defaults to SCENE_CONTEXT when types is omitted", async () => {
    const result = await exec(getContext, { types: ["SCENE_CONTEXT"] });
    expect(result).toContain("SCENE CONTEXT");
  });

  it("handles all seven types at once", async () => {
    const result = await exec(getContext, {
      types: [
        "SCENE_CONTEXT",
        "CHARACTERS_BRIEF",
        "LOCATIONS_BRIEF",
        "OBJECTS_BRIEF",
        "PLOTS_BRIEF",
        "SCHEMA_DUMP",
        "RELATIONSHIP_DUMP",
      ],
    });
    expect(result).toContain("SCENE CONTEXT");
    expect(result).toContain("## CHARACTERS");
    expect(result).toContain("## LOCATIONS");
    expect(result).toContain("## OBJECTS");
    expect(result).toContain("## PLOTS");
    expect(result).toContain("## Schema");
    expect(result).toContain("## RELATIONSHIPS");
  });
});
