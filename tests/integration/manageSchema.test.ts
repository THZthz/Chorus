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
  });
});
