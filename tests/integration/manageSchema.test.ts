import { manageSchema } from "@/server/llm/tools/manageSchema";
import { exec } from "../helpers";
import { resetDb } from "../helpers";

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
        sourceLabels: ["Entity", "Character"],
        targetLabels: ["Entity", "Location"],
      });
      expect(result).toContain("Registered relationship type");
      expect(result).toContain("TEST_CONNECTS_TO");
    });

    it("registers a relationship type without endpoint constraints", async () => {
      const result = await exec(manageSchema, {
        target: "relationship",
        action: "register",
        name: "TEST_GENERIC",
        description: "A generic test relationship",
      });
      expect(result).toContain("Registered relationship type");
    });

    it("unregisters a GM_DEFINED relationship type", async () => {
      // Register first
      await exec(manageSchema, {
        target: "relationship",
        action: "register",
        name: "TEST_TEMP_REL",
        description: "Temporary",
      });

      const result = await exec(manageSchema, {
        target: "relationship",
        action: "unregister",
        name: "TEST_TEMP_REL",
      });
      expect(result).toContain("Unregistered relationship type");
    });

    it("rejects unregister of PREDEFINED relationship type", async () => {
      const result = await exec(manageSchema, {
        target: "relationship",
        action: "unregister",
        name: "LOCATED_AT",
      });
      expect(result).toContain("Cannot unregister");
    });
  });
});
