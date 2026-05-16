import { MemoryClient } from "@/server/memory/client";
import { CypherValidator } from "@/server/memory/validation";
import { stripHiddenProperties } from "@/server/memory/neo4j";
import { getSchemaVisualization, getRelationshipTypeDescriptions, formatSchemaMarkdown } from "@/server/models/schema";
import { jsonrepair } from "jsonrepair";

const AUTO_LIMIT = 50;
const MAX_FORMAT_ROWS = 25;
const DEFAULT_LLM_URL = "http://localhost:8080/v1/chat/completions";

// ── Formatting prompt (lightweight — no schema, just instructions) ──

const FORMATTING_SYSTEM_PROMPT = `
You format raw Neo4j query results into readable markdown for a Game Master.

Respond with ONLY this JSON (no markdown fences, no extra text):
{"markdown": "<human-readable markdown>"}

Rules:
- Use tables for structured multi-row data: | Col1 | Col2 |\\n|------|------|\\n| ... |
- Use bullet lists for simple name-value pairs.
- Omit internal fields (_id, _embedding, _elementId, _labels). Show only meaningful data.
- If the rows are empty, write "(no results)".
- Be concise — the GM needs to scan this quickly.
`.trim();

// ── Types ──

interface TranslateResult {
  query: string;
  explanation: string;
}

interface FormatResult {
  markdown: string;
}

export interface CypherTranslatorResult {
  query: string;
  explanation: string;
  markdown: string;
  rawRows: Record<string, unknown>[];
  rowCount: number;
}

export interface CypherTranslatorBatchResult {
  intent: string;
  result?: CypherTranslatorResult;
  error?: string;
}

// ── System prompt builder ──

export async function buildCypherTranslatorSystemPrompt(): Promise<string> {
  const client = MemoryClient.getCachedInstance();
  const db = client.neo4j;

  const [schemaVis, relTypeDescs] = await Promise.all([
    getSchemaVisualization(db),
    getRelationshipTypeDescriptions(db),
  ]);

  const schemaSection = formatSchemaMarkdown(schemaVis, relTypeDescs);

  return `
You are a precise Cypher query generator for a Neo4j-backed RPG engine.
Given a natural-language intent, produce a read-only Cypher query.

${schemaSection}

## RULES

- Read-only ONLY (MATCH, RETURN, ORDER BY, LIMIT, WHERE, WITH, OPTIONAL MATCH, COLLECT).
- Use ONLY the labels, properties, and relationship types listed in the Schema above.
- Use \`COLLECT { }\` subqueries for fetching lists (1-to-many). Use \`OPTIONAL MATCH\` only for single optional links. Never chain independent \`OPTIONAL MATCH\` clauses.
- Never use unbounded variable-length paths like \`(*)\` or \`[*]\`. Use a fixed upper bound like \`[*1..5]\`.
- \`_\`-prefixed properties (_id, _embedding, _elementId, _labels, _type) are internal. Never return them.
- Entity names are natural keys — use \`{name: "..."}\` to match entities, not \`{_id: "..."}\`.
- The Player entity has \`name: "Player"\`.

## OUTPUT

Respond with ONLY this JSON (no markdown fences, no extra text):
{"query": "<the Cypher query>", "explanation": "<1-sentence summary of what this queries>"}
`.trim();
}

// ── LLM helpers ──

async function chatJson<T>(
  llmUrl: string,
  systemPrompt: string,
  userMessage: string,
  maxTokens: number = 2048,
): Promise<T> {
  const res = await fetch(llmUrl, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      messages: [
        { role: "system", content: systemPrompt },
        { role: "user", content: userMessage },
      ],
      temperature: 0,
      max_tokens: maxTokens,
      response_format: { type: "json_object" },
    }),
  });

  if (!res.ok) {
    const body = await res.text().catch(() => "");
    throw new Error(`LLM server returned ${res.status}: ${body.slice(0, 200)}`);
  }

  const data = (await res.json()) as {
    choices?: Array<{ message?: { content?: string } }>;
  };
  const content = data?.choices?.[0]?.message?.content;
  if (!content) throw new Error("Empty response from LLM");

  // Strip markdown code fences if present
  let json = content.trim();
  if (json.startsWith("```")) {
    json = json.replace(/^```(?:json)?\s*\n?/, "").replace(/\n?```\s*$/, "");
  }

  try {
    return JSON.parse(json) as T;
  } catch (parseErr) {
    const parseMsg = parseErr instanceof Error ? parseErr.message : String(parseErr);
    console.error("[cypherTranslator] JSON parse failed:", parseMsg);
    console.error("[cypherTranslator] Raw content (first 500 chars):", json.slice(0, 500));
    try {
      const repaired = jsonrepair(json);
      console.error("[cypherTranslator] Repaired JSON (first 300 chars):", repaired.slice(0, 300));
      return JSON.parse(repaired) as T;
    } catch (repairErr) {
      const repairMsg = repairErr instanceof Error ? repairErr.message : String(repairErr);
      throw new Error(`LLM returned unparseable JSON. Parse error: ${parseMsg}. Repair error: ${repairMsg}. Raw (first 200 chars): ${json.slice(0, 200)}`);
    }
  }
}

// ── Round 1: Intent → Cypher ──

async function translateIntent(
  llmUrl: string,
  systemPrompt: string,
  intent: string,
): Promise<TranslateResult> {
  const result = await chatJson<{ query?: string; explanation?: string }>(
    llmUrl,
    systemPrompt,
    `Intent: ${intent}`,
  );
  if (!result.query) throw new Error("LLM did not return a query");
  return {
    query: result.query,
    explanation: result.explanation || "Query executed.",
  };
}

// ── Round 2: Raw rows → Markdown ──

async function formatResult(
  llmUrl: string,
  intent: string,
  query: string,
  rows: Record<string, unknown>[],
): Promise<string> {
  const truncated = rows.length > MAX_FORMAT_ROWS;
  const displayRows = truncated ? rows.slice(0, MAX_FORMAT_ROWS) : rows;

  const userMessage = `The user asked: "${intent}"

The Cypher query executed was:
\`\`\`cypher
${query}
\`\`\`

Raw results (${rows.length} rows${truncated ? `, showing first ${MAX_FORMAT_ROWS}` : ""}):
\`\`\`json
${JSON.stringify(displayRows, null, 2)}
\`\`\`
${truncated ? `(${rows.length - MAX_FORMAT_ROWS} more rows not shown)` : ""}

Format these results as readable markdown.`;

  const result = await chatJson<{ markdown?: string }>(
    llmUrl,
    FORMATTING_SYSTEM_PROMPT,
    userMessage,
    4096,
  );
  let markdown = result.markdown || "(no output)";
  if (truncated) {
    markdown += `\n\n*(Showing ${MAX_FORMAT_ROWS} of ${rows.length} rows)*`;
  }
  return markdown;
}

// ── Orchestrator ──

export async function executeCypherTranslator(
  llmUrl: string,
  intent: string,
): Promise<CypherTranslatorResult> {
  const systemPrompt = await buildCypherTranslatorSystemPrompt();

  // Round 1: generate Cypher
  const { query, explanation } = await translateIntent(llmUrl, systemPrompt, intent);

  // Validate
  const validator = new CypherValidator();
  const validation = validator.validateRead(query);
  if (!validation.valid) {
    throw new Error(`Generated query failed validation: ${validation.errors.join("; ")}.\nQuery: ${query}`);
  }

  // Auto-limit
  let finalQuery = query.trim();
  if (!/\bLIMIT\b/i.test(finalQuery)) {
    finalQuery = `${finalQuery} LIMIT ${AUTO_LIMIT}`;
  }

  // EXPLAIN check
  const client = MemoryClient.getCachedInstance();
  try {
    await client.neo4j.executeRead(`EXPLAIN ${finalQuery}`);
  } catch (explainErr) {
    const msg = explainErr instanceof Error ? explainErr.message : String(explainErr);
    throw new Error(`Generated query has syntax error: ${msg}.\nQuery: ${finalQuery}`);
  }

  // Execute
  const rows = await client.neo4j.executeRead(finalQuery);
  const safeRows = stripHiddenProperties(rows) as Record<string, unknown>[];

  // Round 2: format results as markdown (non-fatal if it fails)
  let markdown = "";
  try {
    markdown = await formatResult(llmUrl, intent, finalQuery, safeRows);
  } catch (formatErr) {
    const msg = formatErr instanceof Error ? formatErr.message : String(formatErr);
    console.error("[cypherTranslator] formatResult failed:", msg);
    markdown = `(Formatting failed: ${msg})`;
  }

  return {
    query: finalQuery,
    explanation,
    markdown,
    rawRows: safeRows,
    rowCount: safeRows.length,
  };
}

// ── Batch orchestrator ──

async function processOneIntent(
  llmUrl: string,
  systemPrompt: string,
  intent: string,
): Promise<CypherTranslatorBatchResult> {
  try {
    // Round 1: generate Cypher
    const { query, explanation } = await translateIntent(llmUrl, systemPrompt, intent);

    // Validate
    const validator = new CypherValidator();
    const validation = validator.validateRead(query);
    if (!validation.valid) {
      return { intent, error: `Query failed validation: ${validation.errors.join("; ")}` };
    }

    // Auto-limit
    let finalQuery = query.trim();
    if (!/\bLIMIT\b/i.test(finalQuery)) {
      finalQuery = `${finalQuery} LIMIT ${AUTO_LIMIT}`;
    }

    // EXPLAIN check
    const client = MemoryClient.getCachedInstance();
    try {
      await client.neo4j.executeRead(`EXPLAIN ${finalQuery}`);
    } catch (explainErr) {
      const msg = explainErr instanceof Error ? explainErr.message : String(explainErr);
      return { intent, error: `Generated query has syntax error: ${msg}` };
    }

    // Execute
    const rows = await client.neo4j.executeRead(finalQuery);
    const safeRows = stripHiddenProperties(rows) as Record<string, unknown>[];

    // Round 2: format results (non-fatal)
    let markdown = "";
    try {
      markdown = await formatResult(llmUrl, intent, finalQuery, safeRows);
    } catch (formatErr) {
      const msg = formatErr instanceof Error ? formatErr.message : String(formatErr);
      console.error("[cypherTranslator] formatResult failed for intent:", intent.slice(0, 80), msg);
      markdown = `(Formatting failed: ${msg})`;
    }

    return {
      intent,
      result: { query: finalQuery, explanation, markdown, rawRows: safeRows, rowCount: safeRows.length },
    };
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    console.error("[cypherTranslator] intent failed:", intent.slice(0, 80), msg);
    return { intent, error: msg };
  }
}

export async function executeCypherTranslatorBatch(
  llmUrl: string,
  intents: string[],
): Promise<CypherTranslatorBatchResult[]> {
  const systemPrompt = await buildCypherTranslatorSystemPrompt();

  const results = await Promise.all(
    intents.map((intent) => processOneIntent(llmUrl, systemPrompt, intent)),
  );

  return results;
}

export { DEFAULT_LLM_URL };
