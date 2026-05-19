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

import { manageSchema } from "@/server/llm/tools/manageSchema";
import { exec, resetDb } from "../helpers";

describe("manageSchema", () => {
  beforeEach(async () => {
    await resetDb();
  });

  describe("node types", () => {
    it("registers a new node type with property schema", async () => {
      const result = await exec(manageSchema, {
        target: "node",
        action: "register",
        name: "TestArtifact",
        description: "A test artifact node type",
        properties: [
          { name: "power", description: "Power level", tags: ["number"] },
          { name: "origin", description: "Place of origin", tags: ["string"] },
        ],
      });
      expect(result).toContain("Registered node type");
      expect(result).toContain("TestArtifact");
      expect(result).toContain("power");
      expect(result).toContain("origin");
    });

    it("unregisters a GM_DEFINED node type", async () => {
      // Register first
      await exec(manageSchema, {
        target: "node",
        action: "register",
        name: "TempType",
        description: "Temporary test type",
      });

      const result = await exec(manageSchema, {
        target: "node",
        action: "unregister",
        name: "TempType",
      });
      expect(result).toContain("Unregistered node type");
      expect(result).toContain("TempType");
    });

    it("rejects unregister of PREDEFINED type", async () => {
      const result = await exec(manageSchema, {
        target: "node",
        action: "unregister",
        name: "Entity",
      });
      expect(result).toContain("Cannot unregister");
    });
  });

  describe("relationship types", () => {
    it("registers a new relationship type with endpoint constraints", async () => {
      const result = await exec(manageSchema, {
        target: "relationship",
        action: "register",
        name: "TEST_CONNECTS_TO",
        description: "Test connection between entities",
        sourceLabel: "Entity",
        targetLabel: "Location",
      });
      expect(result).toContain("Registered relationship type");
      expect(result).toContain("TEST_CONNECTS_TO");
      expect(result).toContain("(Entity)→(Location)");
    });

    it("rejects registration without sourceLabel and targetLabel", async () => {
      const result = await exec(manageSchema, {
        target: "relationship",
        action: "register",
        name: "TEST_GENERIC",
        description: "A generic test relationship",
      });
      expect(result).toContain("ERROR");
    });

    it("unregisters a GM_DEFINED relationship type", async () => {
      // Register first
      await exec(manageSchema, {
        target: "relationship",
        action: "register",
        name: "TEST_TEMP_REL",
        description: "Temporary",
        sourceLabel: "Entity",
        targetLabel: "Entity",
      });

      const result = await exec(manageSchema, {
        target: "relationship",
        action: "unregister",
        name: "TEST_TEMP_REL",
        sourceLabel: "Entity",
        targetLabel: "Entity",
      });
      expect(result).toContain("Unregistered relationship type");
    });

    it("allows registering same name with different sourceLabel", async () => {
      const r1 = await exec(manageSchema, {
        target: "relationship",
        action: "register",
        name: "TEST_DUAL",
        description: "First variant",
        sourceLabel: "Entity",
        targetLabel: "Entity",
      });
      expect(r1).toContain("Registered relationship type");

      const r2 = await exec(manageSchema, {
        target: "relationship",
        action: "register",
        name: "TEST_DUAL",
        description: "Second variant",
        sourceLabel: "Entity",
        targetLabel: "Location",
      });
      expect(r2).toContain("Registered relationship type");
      expect(r2).toContain("(Entity)→(Location)");
    });

    it("rejects unregister without sourceLabel/targetLabel", async () => {
      const result = await exec(manageSchema, {
        target: "relationship",
        action: "unregister",
        name: "SOME_TYPE",
      });
      expect(result).toContain("ERROR");
    });

    it("rejects unregister of PREDEFINED relationship type", async () => {
      const result = await exec(manageSchema, {
        target: "relationship",
        action: "unregister",
        name: "LOCATED_AT",
        sourceLabel: "Entity",
        targetLabel: "Location",
      });
      expect(result).toContain("Cannot unregister");
    });

    it("registers a relationship type with embedded, index, and composite_index tags", async () => {
      // Relationships now support the same property tags as nodes except
      // 'unique' — Neo4j has no uniqueness constraint for relationship properties.
      const result = await exec(manageSchema, {
        target: "relationship",
        action: "register",
        name: "TEST_TAGGED_REL",
        description: "Relationship with full property tags",
        sourceLabel: "Entity",
        targetLabel: "Entity",
        properties: [
          {
            name: "summary",
            description: "Summary text for embedding",
            tags: ["string", "embedded"],
          },
          {
            name: "weight",
            description: "Relationship weight for indexing",
            tags: ["number", "index"],
          },
          {
            name: "group_key",
            description: "First composite key",
            tags: ["string", "composite_index_1"],
          },
          {
            name: "sub_key",
            description: "Second composite key",
            tags: ["string", "composite_index_1"],
          },
        ],
      });
      expect(result).toContain("Registered relationship type");
      expect(result).toContain("TEST_TAGGED_REL");
      expect(result).toContain("summary");
      expect(result).toContain("weight");

      // Cleanup
      await exec(manageSchema, {
        target: "relationship",
        action: "unregister",
        name: "TEST_TAGGED_REL",
        sourceLabel: "Entity",
        targetLabel: "Entity",
      });
    });
  });
});
