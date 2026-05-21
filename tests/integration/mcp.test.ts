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

import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { InMemoryTransport } from "@modelcontextprotocol/sdk/inMemory.js";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";

import { queryWorld } from "@/server/llm/tools/queryWorld";
import { searchWorld } from "@/server/llm/tools/searchWorld";
import { editNode } from "@/server/llm/tools/editNode";
import { editRelationship } from "@/server/llm/tools/editRelationship";
import { editNote } from "@/server/llm/tools/editNote";
import { editPlot } from "@/server/llm/tools/editPlot";
import { getContext } from "@/server/llm/tools/getContext";
import { manageSchema } from "@/server/llm/tools/manageSchema";
import { createGenerateDialogueStepTool } from "@/server/llm/tools/generateDialogueStep";
import { createAdvanceTimeTool } from "@/server/llm/tools/advanceTime";
import { createMockEventEmitter, resetDb } from "../helpers";
import { setupServer } from "@/server/mcp.ts";
import { TOOL_NAMES } from "@/shared/constants.ts";

function buildServer(): McpServer {
  const dialogueStepTool = createGenerateDialogueStepTool();
  const advanceTimeTool = createAdvanceTimeTool(createMockEventEmitter());

  const server = new McpServer({ name: "chorus-gm-test", version: "1.0.0" });

  setupServer(server, dialogueStepTool, advanceTimeTool);

  return server;
}

describe("MCP Server", () => {
  let server: McpServer;
  let client: Client;
  let serverTransport: InMemoryTransport;
  let clientTransport: InMemoryTransport;

  beforeAll(async () => {
    await resetDb();
  });

  beforeEach(async () => {
    server = buildServer();
    [clientTransport, serverTransport] = InMemoryTransport.createLinkedPair();
    client = new Client({ name: "test-client", version: "1.0.0" }, { capabilities: {} });
    await Promise.all([
      server.connect(serverTransport),
      client.connect(clientTransport),
    ]);
  });

  afterEach(async () => {
    await client.close();
    await server.close();
  });

  // ── Tool registration ──

  describe("tool registration", () => {
    it("registers all 10 GM tools", async () => {
      const { tools } = await client.listTools();
      const names = tools.map((t: any) => t.name).sort();

      expect(names).toEqual([
        TOOL_NAMES.ADVANCE_TIME,
        TOOL_NAMES.EDIT_NODE,
        TOOL_NAMES.EDIT_NOTE,
        TOOL_NAMES.EDIT_PLOT,
        TOOL_NAMES.EDIT_RELATIONSHIP,
        TOOL_NAMES.GENERATE_DIALOGUE,
        TOOL_NAMES.GET_CONTEXT,
        TOOL_NAMES.MANAGE_SCHEMA,
        TOOL_NAMES.QUERY_WORLD,
        TOOL_NAMES.SEARCH_WORLD,
      ]);
    });

    it("each tool has a non-empty description", async () => {
      const { tools } = await client.listTools();
      for (const tool of tools) {
        expect(tool.description).toBeTruthy();
      }
    });

    it("each tool exposes an inputSchema", async () => {
      const { tools } = await client.listTools();
      for (const tool of tools) {
        expect(tool.inputSchema).toBeTruthy();
        expect(tool.inputSchema.type).toBe("object");
      }
    });
  });

  // ── Tool execution ──

  describe("tool execution", () => {
    it("queryWorld: reads entities from seed data", async () => {
      const result = await client.callTool({
        name: "queryWorld",
        arguments: { action: "READ", query: "MATCH (e:Entity) RETURN e.name, e.type LIMIT 3" },
      });

      const text = (result as any).content?.[0]?.text;
      expect(text).toBeTruthy();
      const data = JSON.parse(text!);
      expect(data.rowCount).toBeGreaterThan(0);
      expect(Array.isArray(data.rows)).toBe(true);
    });

    it("getContext: returns scene context by default", async () => {
      const result = await client.callTool({
        name: "getContext",
        arguments: { types: ["SCENE_CONTEXT"] },
      });

      const text = (result as any).content?.[0]?.text;
      expect(text).toBeTruthy();
      expect(typeof text).toBe("string");
      expect(text!.length).toBeGreaterThan(0);
    });

    it("searchWorld: searches entities via vector search", async () => {
      const result = await client.callTool({
        name: "searchWorld",
        arguments: { query: "tavern", target: ["node"], domains: ["Entity"], limit: 3 },
      });

      const text = (result as any).content?.[0]?.text;
      expect(text).toBeTruthy();
      // searchWorld returns JSON mapping domain→results
      const data = JSON.parse(text!);
      expect(data).toHaveProperty("Entity");
    });

    it("manageSchema: rejects duplicate PREDEFINED type registration", async () => {
      const result = await client.callTool({
        name: "manageSchema",
        arguments: {
          target: "NODE",
          action: "REGISTER",
          name: "Entity",
          description: "Should fail",
        },
      });

      const text = (result as any).content?.[0]?.text;
      expect(text).toContain("ERROR");
    });

    it("generateDialogueStep: validates and rejects empty messages", async () => {
      const result = await client.callTool({
        name: "generateDialogueStep",
        arguments: { messages: [], options: [] },
      });

      const text = (result as any).content?.[0]?.text;
      expect(text).toContain("VALIDATION FAILED");
    });

    it("advanceTime: records time advancement", async () => {
      const result = await client.callTool({
        name: "advanceTime",
        arguments: { hours: 1, reason: "Test time advance" },
      });

      const text = (result as any).content?.[0]?.text;
      expect(text).toContain("Time advanced");
    });
  });

  // ── Content format ──

  describe("content format", () => {
    it("tool results follow MCP content format", async () => {
      const result = await client.callTool({
        name: "queryWorld",
        arguments: { action: "READ", query: "MATCH (e:Entity) RETURN e.name LIMIT 1" },
      });

      const r = result as any;
      expect(Array.isArray(r.content)).toBe(true);
      expect(r.content[0].type).toBe("text");
      expect(typeof r.content[0].text).toBe("string");
    });

    it("queryWorld WRITE returns success message", async () => {
      const result = await client.callTool({
        name: "queryWorld",
        arguments: {
          action: "WRITE",
          query:
            "MERGE (e:Entity {_id: 'test-mcp-entity', name: 'MCP Test', type: 'Object'}) SET e.description = 'Created by MCP test'",
        },
      });

      const text = (result as any).content?.[0]?.text;
      expect(text).toContain("Success");
    });
  });
});
