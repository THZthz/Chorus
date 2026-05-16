import { MemoryClient } from "@/server/memory/client";
import { CypherValidator } from "@/server/memory/validation";
import { stripHiddenProperties } from "@/server/memory/neo4j";
import { getSchemaVisualization, getRelationshipTypeDescriptions, formatSchemaMarkdown } from "@/server/models/schema";

const AUTO_LIMIT = 50;
const MAX_FORMAT_ROWS = 25;
const DEFAULT_LLM_URL = "http://localhost:8080/v1/chat/completions";

// ── Formatting prompt (round 2) — the entire response IS the markdown ──

const FORMATTING_SYSTEM_PROMPT = `
You format raw Neo4j query results into readable markdown for a Game Master.

Your ENTIRE response is the markdown. Do NOT wrap it in code fences, JSON, TOML, or any other container. Just output the markdown directly.

Rules:
- Start with a level-2 heading describing what the data shows.
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

// ── System prompt builder (round 1) ──

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

## OUTPUT FORMAT

Respond with EXACTLY this structure (no code fences):

<<<QUERY>>>
<the Cypher query here>
<<<EXPLANATION>>>
<1-sentence summary of what this queries>
`.trim();
}

// ── LLM helpers ──

function parseQueryResponse(raw: string): TranslateResult {
  const cleaned = raw.trim();

  const queryMatch = cleaned.match(/<<<QUERY>>>\s*([\s\S]*?)<<<EXPLANATION>>>/i);
  const query = queryMatch?.[1]?.trim() || "";

  const explanationIdx = cleaned.lastIndexOf("<<<EXPLANATION>>>");
  let explanation = "";
  if (explanationIdx !== -1) {
    explanation = cleaned.slice(explanationIdx + "<<<EXPLANATION>>>".length).trim();
  }

  return { query, explanation };
}

async function llmChat(
  llmUrl: string,
  systemPrompt: string,
  userMessage: string,
  maxTokens: number = 2048,
): Promise<string> {
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

  // Strip code fences if present
  let text = content.trim();
  if (text.startsWith("```")) {
    text = text.replace(/^```(?:toml|json|markdown|md)?\s*\n?/, "").replace(/\n?```\s*$/, "");
  }
  return text;
}

// ── Round 1: Intent → Cypher ──

async function translateIntent(
  llmUrl: string,
  systemPrompt: string,
  intent: string,
): Promise<TranslateResult> {
  const raw = await llmChat(llmUrl, systemPrompt, `Intent: ${intent}`);
  const result = parseQueryResponse(raw);
  if (!result.query) {
    console.error("[cypherTranslator] parseQueryResponse failed, raw (first 500):", raw.slice(0, 500));
    throw new Error("LLM did not return a parseable query. Ensure <<<QUERY>>> and <<<EXPLANATION>>> tags are present.");
  }
  return result;
}

// ── Round 2: Raw rows → Markdown (entire response IS the markdown) ──

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

  let markdown = await llmChat(llmUrl, FORMATTING_SYSTEM_PROMPT, userMessage, 4096);
  if (!markdown || markdown === "(no results)") markdown = "(no output)";
  if (truncated) {
    markdown += `\n\n*(Showing ${MAX_FORMAT_ROWS} of ${rows.length} rows)*`;
  }
  return markdown;
}

// ── Single-intent orchestrator ──

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
    const { query, explanation } = await translateIntent(llmUrl, systemPrompt, intent);

    const validator = new CypherValidator();
    const validation = validator.validateRead(query);
    if (!validation.valid) {
      return { intent, error: `Query failed validation: ${validation.errors.join("; ")}` };
    }

    let finalQuery = query.trim();
    if (!/\bLIMIT\b/i.test(finalQuery)) {
      finalQuery = `${finalQuery} LIMIT ${AUTO_LIMIT}`;
    }

    const client = MemoryClient.getCachedInstance();
    try {
      await client.neo4j.executeRead(`EXPLAIN ${finalQuery}`);
    } catch (explainErr) {
      const msg = explainErr instanceof Error ? explainErr.message : String(explainErr);
      return { intent, error: `Generated query has syntax error: ${msg}` };
    }

    const rows = await client.neo4j.executeRead(finalQuery);
    const safeRows = stripHiddenProperties(rows) as Record<string, unknown>[];

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
