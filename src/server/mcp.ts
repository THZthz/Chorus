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
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";

// Plain singleton tools — no factory needed
import { queryWorld } from "@/server/llm/tools/queryWorld";
import { searchWorld } from "@/server/llm/tools/searchWorld";
import { editNode } from "@/server/llm/tools/editNode";
import { editRelationship } from "@/server/llm/tools/editRelationship";
import { editNote } from "@/server/llm/tools/editNote";
import { editPlot } from "@/server/llm/tools/editPlot";
import { getContext } from "@/server/llm/tools/getContext";
import { manageSchema } from "@/server/llm/tools/manageSchema";

// Factory-based tools — instantiate for MCP context
import { createGenerateDialogueStepTool } from "@/server/llm/tools/generateDialogueStep";
import { createAdvanceTimeTool } from "@/server/llm/tools/advanceTime";

const dialogueStepTool = createGenerateDialogueStepTool();
const advanceTimeTool = createAdvanceTimeTool({
  emitTimeUpdate: () => {},
} as any);

/**
 * Wrap a Vercel AI SDK tool's execute() for MCP registerTool().
 *
 * The Vercel AI SDK and MCP SDK use incompatible Zod type wrappers, so we
 * cast through `any` at the bridge points (inputSchema and the tool object).
 */
function wrap(execute: (args: any) => Promise<string>) {
  return async (args: any) => ({
    content: [{ type: "text" as const, text: await execute(args) }],
  });
}

const server = new McpServer({
  name: "chorus-gm",
  version: "1.0.0",
});

function reg(name: string, desc: string, schema: unknown, execute: (args: any) => Promise<string>) {
  server.registerTool(
    name,
    {
      description: desc,
      inputSchema: schema as any, // Vercel AI SDK Zod v4 <-> MCP SDK Zod compat
    },
    wrap(execute),
  );
}

reg("queryWorld", queryWorld.description!, queryWorld.inputSchema, queryWorld.execute as any);
reg("searchWorld", searchWorld.description!, searchWorld.inputSchema, searchWorld.execute as any);
reg("manageSchema", manageSchema.description!, manageSchema.inputSchema, manageSchema.execute as any);
reg("editNode", editNode.description!, editNode.inputSchema, editNode.execute as any);
reg("editRelationship", editRelationship.description!, editRelationship.inputSchema, editRelationship.execute as any);
reg("editNote", editNote.description!, editNote.inputSchema, editNote.execute as any);
reg("editPlot", editPlot.description!, editPlot.inputSchema, editPlot.execute as any);
reg("getContext", getContext.description!, getContext.inputSchema, getContext.execute as any);
reg("generateDialogueStep", dialogueStepTool.tool.description!, dialogueStepTool.tool.inputSchema, dialogueStepTool.tool.execute as any);
reg("advanceTime", advanceTimeTool.description!, advanceTimeTool.inputSchema, advanceTimeTool.execute as any);

async function main() {
  const transport = new StdioServerTransport();
  await server.connect(transport);
  console.error("[chorus-gm] MCP server ready on stdio");
}

main().catch((err) => {
  console.error("[chorus-gm] Fatal:", err);
  process.exit(1);
});
