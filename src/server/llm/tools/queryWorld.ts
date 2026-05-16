/**
 * Chorus — cinematic RPG-style dialogue engine
 * Copyright (C) 2026  Amias
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

import { tool } from "ai";
import { z } from "zod";
import { MemoryClient } from "@/server/memory/client";
import { CypherValidator } from "@/server/memory/validation";
import { stripHiddenProperties } from "@/server/memory/neo4j";
import { wrapSafe } from "@/server/llm/tools/shared";
import { TOOL_NAMES } from "@/shared/constants";

const validator = new CypherValidator();
const AUTO_LIMIT = 50;

const LLAMA_FORMATTER_URL =
  process.env.LLAMA_FORMATTER_URL || "http://localhost:8082/v1/chat/completions";
const LLAMA_FORMATTER_MODEL = process.env.LLAMA_FORMATTER_MODEL || "phi-4-mini-instruct";

async function formatWithLocalLLM(instruction: string, queryResult: string): Promise<string> {
  const systemPrompt = [
    "You are a data-formatting assistant for a cinematic RPG game engine.",
    "Your task is to format raw Neo4j Cypher query results into readable Markdown for the Game Master.",
    "",
    "The data comes from a fantasy-steampunk game world. Nodes represent entities (characters,",
    "locations, objects, organizations, events), messages, notes, plots, dispositions, and time points.",
    "Properties use snake_case names. Internal properties prefixed with underscore are already stripped.",
    "",
    "Guidelines:",
    "- Present the data clearly using Markdown (tables, lists, headings as appropriate).",
    "- Be concise. Do not add narrative flourishes or roleplay.",
    "- Do not invent or assume data not present in the result.",
    "- If the result is empty, state that clearly.",
    "- If the instruction asks for a summary, distill the key points without losing critical detail.",
    "",
    `GM's formatting instruction: ${instruction}`,
  ].join("\n");

  const res = await fetch(LLAMA_FORMATTER_URL, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      model: LLAMA_FORMATTER_MODEL,
      messages: [
        { role: "system", content: systemPrompt },
        { role: "user", content: queryResult },
      ],
      temperature: 0,
    }),
  });
  if (!res.ok) {
    const body = await res.text().catch(() => "");
    throw new Error(`Local LLM returned ${res.status}: ${body.slice(0, 200)}`);
  }
  const json = (await res.json()) as Record<string, unknown>;
  const content = (json.choices as Array<{ message?: { content?: string } }>)?.[0]?.message?.content;
  if (content) return content;
  throw new Error("Local LLM returned empty response");
}

export const queryWorld = tool({
  title: TOOL_NAMES.QUERY_WORLD,
  description: `
Read the game world using Cypher queries.
The query MUST be read-only (MATCH, RETURN, ORDER BY, LIMIT), otherwise it will be rejected by validator of the tool.
Use MATCH patterns to navigate relationships like LOCATED_AT, CARRIES, ALLIED_WITH, HOSTILE_TOWARDS.
Entity types: CHARACTER, OBJECT, LOCATION, ORGANIZATION. Current time: MATCH (a:TimeAnchor {_id:'anchor'})-[:CURRENT_TIMEPOINT]->(tp:TimePoint) RETURN tp.day, tp.segment, tp.label.
Browse time history via NEXT_TIMEPOINT.

NOTE:
The current scene (player location, nearby NPCs, objects, inventory, NPC dispositions, and active plots) will be pre-loaded in the user prompt under section "SCENE CONTEXT".
Do NOT query for scene information that is already present.
Use ${TOOL_NAMES.QUERY_WORLD} only for specific lookups BEYOND the pre-loaded context, such as: entity searches by name, message history, timepoint browsing, or finding entities/relationships not shown in the scene.
Some internal properties prefixed with "_" will not shown in the result JSON.

When rawResult is set to false, the query result will be formatted by a local LLM according to the instruction.
Use instruction to specify how the result should be presented (e.g. "Format as a markdown table", "Summarize into a paragraph", "List only the names").
`.trim(),
  inputSchema: z.object({
    query: z.string().describe("A read-only Cypher query (MATCH...RETURN)."),
    instruction: z
      .string()
      .nullable()
      .optional()
      .describe(
        "Instruction for formatting the query result via a local LLM. Only used when rawResult is false.",
      ),
    rawResult: z
      .boolean()
      .nullable()
      .optional()
      .describe(
        "When false, sends the query result to a local LLM with the instruction for formatting. Default true (return raw JSON).",
      ),
  }),
  execute: wrapSafe(async (args) => {
    const validation = validator.validateRead(args.query);
    if (!validation.valid) {
      return `VALIDATION FAILED:\n${validation.errors.join("; ")}.\nRewrite your query and retry.`;
    }

    let query = args.query.trim();
    if (!/\bLIMIT\b/i.test(query)) {
      query = `${query} LIMIT ${AUTO_LIMIT}`;
    }

    const client = MemoryClient.getCachedInstance();
    try {
      try {
        await client.neo4j.executeRead(`EXPLAIN ${query}`);
      } catch (explainErr) {
        const msg = explainErr instanceof Error ? explainErr.message : String(explainErr);
        return `CYPHER SYNTAX ERROR:\n${msg}.\nFix your query and retry.`;
      }

      const rows = await client.neo4j.executeRead(query);
      const safeRows = stripHiddenProperties(rows);
      const rawResult = args.rawResult ?? true;
      const instruction = args.instruction;

      if (!rawResult && instruction) {
        try {
          const resultJson = JSON.stringify({ rowCount: safeRows.length, rows: safeRows });
          const formatted = await formatWithLocalLLM(instruction, resultJson);
          return `Formatted result:\n${formatted}`;
        } catch (fmtErr) {
          const msg = fmtErr instanceof Error ? fmtErr.message : String(fmtErr);
          return `FORMATTER ERROR: ${msg}.\nRaw result:\n${JSON.stringify({ rowCount: safeRows.length, rows: safeRows }, null, 2)}`;
        }
      }

      return JSON.stringify({ rowCount: safeRows.length, rows: safeRows }, null, 2);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      return `QUERY ERROR:\n${msg}.\nAdjust your query and retry.`;
    }
  }, TOOL_NAMES.QUERY_WORLD),
});
