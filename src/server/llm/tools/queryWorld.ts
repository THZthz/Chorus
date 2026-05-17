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
import { resetEntityForQuery } from "@/server/llm/sceneObserver";
import { TOOL_NAMES } from "@/shared/constants";

const validator = new CypherValidator();
const AUTO_LIMIT = 50;
const REASONING_BUDGET: Record<string, number> = { simple: 0, normal: 512, hard: 1024 };

const LLAMA_FORMATTER_URL =
  process.env.LLAMA_FORMATTER_URL || "http://localhost:8082/v1/chat/completions";

async function formatWithLocalLLM(
  instruction: string,
  queryResult: string,
  reasoningBudget?: number,
): Promise<string> {
  const systemPrompt = [
    "Your only task is to restructure JSON into Markdown. Output result ONLY.",
    "",
    "Rules:",
    "- Present every row exactly as given. Never omit, merge, or rewrite data.",
    "- NEVER add information not present in the input.",
    "- If the input is empty, output 'No results.'",
    "",
    "The input is JSON with snake_case keys. Format it according to the user's instruction:",
    "",
    instruction,
  ].join("\n");

  const res = await fetch(LLAMA_FORMATTER_URL, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      model: "model",
      messages: [
        { role: "system", content: systemPrompt },
        { role: "user", content: queryResult },
      ],
      temperature: 0,
      ...(reasoningBudget != null ? { reasoning_budget: reasoningBudget } : {}),
    }),
  });
  if (!res.ok) {
    const body = await res.text().catch(() => "");
    throw new Error(`Local LLM returned ${res.status}: ${body.slice(0, 200)}`);
  }
  const json = (await res.json()) as Record<string, unknown>;
  const content = (json.choices as Array<{ message?: { content?: string } }>)?.[0]?.message
    ?.content;
  if (content) return content;
  throw new Error("Local LLM returned empty response");
}

export const queryWorld = tool({
  title: TOOL_NAMES.QUERY_WORLD,
  description: `
Query the game world using Cypher. Use \`action\` to choose the mode:

**READ** (default): Read-only queries (MATCH, RETURN, ORDER BY, LIMIT). Use for lookups.
Use MATCH patterns to navigate relationships like LOCATED_AT, CARRIES, ALLIED_WITH, HOSTILE_TOWARDS.
Entity types: CHARACTER, OBJECT, LOCATION, ORGANIZATION. Current time: MATCH (a:TimeAnchor {_id:'anchor'})-[:CURRENT_TIMEPOINT]->(tp:TimePoint) RETURN tp.day, tp.segment, tp.label.
Browse time history via NEXT_TIMEPOINT.

**WRITE**: Modify the world (CREATE, MERGE, SET, DELETE). Use for mutations.
Use MERGE for upserts. Use SET to update properties. Use DETACH DELETE to remove entities.
Must include a WHERE clause when deleting.
Before using a new node label or relationship type, register it via \`${TOOL_NAMES.MANAGE_SCHEMA}\`.
Never set a 'description' property directly on relationship instances in your Cypher.

NOTE: The current scene (player location, nearby NPCs, objects, inventory, NPC dispositions, and active plots) will be pre-loaded under "SCENE CONTEXT".
Do NOT query for scene information that is already present.
Use ${TOOL_NAMES.QUERY_WORLD} only for specific lookups or mutations BEYOND the pre-loaded context.
Internal properties prefixed with "_" will not be shown in READ results.

When \`rawResult\` is set to false (READ only), the query result will be formatted by a local LLM according to \`instruction\`.
Use \`instruction\` to specify how the result should be presented (e.g. "Format as a markdown table", "Summarize into a paragraph", "List only the names").
`.trim(),
  inputSchema: z.object({
    action: z
      .enum(["READ", "WRITE"])
      .default("READ")
      .describe("READ to query the world, WRITE to modify it."),
    query: z
      .string()
      .describe(
        "A Cypher query. READ: MATCH...RETURN. WRITE: CREATE, MERGE, SET, DELETE. Must include MATCH with WHERE for deletions.",
      ),
    instruction: z
      .string()
      .nullable()
      .optional()
      .describe(
        "READ action only. Formatting via a small local model (Qwen3.5-9B). Only used when rawResult is false.",
      ),
    rawResult: z
      .boolean()
      .nullable()
      .optional()
      .describe(
        "READ action only. When false, sends the query result to a local LLM with the instruction for formatting. Default true (return raw JSON).",
      ),
    reasoning: z
      .enum(["simple", "normal", "hard"])
      .default("simple")
      .nullable()
      .optional()
      .describe(
        "READ action only. Reasoning budget for the local LLM formatter. Should increase based on the task size and complexity. simple = 0, normal = 512, hard = 1024.",
      ),
  }),
  execute: wrapSafe(async (args) => {
    const client = MemoryClient.getCachedInstance();

    if (args.action === "WRITE") {
      const validation = validator.validateWrite(args.query);
      if (!validation.valid) {
        return `VALIDATION FAILED:\n${validation.errors.join("; ")}.\nRewrite your query and retry.`;
      }

      try {
        try {
          await client.neo4j.executeRead(`EXPLAIN ${args.query}`);
        } catch (explainErr) {
          const msg = explainErr instanceof Error ? explainErr.message : String(explainErr);
          return `CYPHER SYNTAX ERROR:\n${msg}.\nFix your query and retry.`;
        }

        const rows = await client.neo4j.executeWrite(args.query);

        try {
          resetEntityForQuery(args.query);
        } catch {
          // Best-effort
        }

        return `Success. ${rows.length} row(s) affected.`;
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        return `QUERY ERROR:\n${msg}.\nAdjust your query and retry.`;
      }
    }

    // READ
    const validation = validator.validateRead(args.query);
    if (!validation.valid) {
      return `VALIDATION FAILED:\n${validation.errors.join("; ")}.\nRewrite your query and retry.`;
    }

    let query = args.query.trim();
    if (!/\bLIMIT\b/i.test(query)) {
      query = `${query} LIMIT ${AUTO_LIMIT}`;
    }

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
          const budget = args.reasoning ? REASONING_BUDGET[args.reasoning] : undefined;
          const formatted = await formatWithLocalLLM(instruction, resultJson, budget);
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
