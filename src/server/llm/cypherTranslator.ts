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

## RELATIONSHIP DIRECTIONS

Every relationship has a fixed direction. Use these exact patterns:

\`\`\`
(Entity)-[:LOCATED_AT]->(Entity)       entity is at a location
(Entity)-[:LOCATED_IN]->(Entity)       entity inside larger location
(Entity)-[:CARRIES]->(Entity)          carrier carries object/entity
(Entity)-[:ALLIED_WITH]->(Entity)      entity allied with another
(Entity)-[:HOSTILE_TOWARDS]->(Entity)  entity hostile toward another
(Entity)-[:CONNECTED_TO]->(Entity)     generic connection
(Entity)-[:HAS_DISPOSITION]->(NPCDisposition)  NPC attitude toward a target

(Conversation)-[:HAS_MESSAGE]->(Message)  conversation owns message
(Conversation)-[:FIRST_MESSAGE]->(Message) head of message list
(Message)-[:NEXT_MESSAGE]->(Message)      next message in order
(Message)-[:AT_TIME]->(TimePoint)         message timestamp

(Plot)-[:BRANCHES_TO]->(Plot)         parent plot branches to child
(Plot)-[:STARTED_AT]->(TimePoint)     plot start time
(Plot)-[:ACTIVE_AT]->(TimePoint)      plot activation time
(Plot)-[:COMPLETED_AT]->(TimePoint)   plot completion time

(TimeAnchor)-[:CURRENT_TIMEPOINT]->(TimePoint)  current game time
(TimePoint)-[:NEXT_TIMEPOINT]->(TimePoint)      time chain

(Note)-[:ABOUT_ENTITY]->(Entity)     note references entity
(Note)-[:ABOUT_MESSAGE]->(Message)   note references message

INTERNAL (never query): _HAS_GM_MESSAGE, _FIRST_GM_MESSAGE, _NEXT_GM_MESSAGE
\`\`\`

NPCDisposition is a NODE LABEL, not a relationship type. NEVER write \`[d:NPCDisposition]\` or \`[:NPCDisposition]\`. The relationship is \`[:HAS_DISPOSITION]\`. Correct patterns:\n\`\`\`\n// One NPC's disposition toward the Player:\nMATCH (npc:Entity {name: "SomeNPC"})-[:HAS_DISPOSITION]->(d:NPCDisposition {target_name: "Player"})\nRETURN d.sentiment, d.summary\n\n// ALL dispositions toward the Player:\nMATCH (d:NPCDisposition {target_name: "Player"})\nRETURN d.npc_name, d.sentiment, d.summary\n\`\`\`

## RULES

- Read-only ONLY (MATCH, RETURN, ORDER BY, LIMIT, WHERE, WITH, OPTIONAL MATCH, COLLECT).
- Use ONLY labels, properties, relationships from the Schema and directions above.
- Use \`COLLECT { }\` subqueries for lists. Use \`OPTIONAL MATCH\` only for single optional links. Never chain independent \`OPTIONAL MATCH\`s.
- Never unbounded variable-length paths. Use a fixed upper bound like \`[*1..5]\`.
- \`_\`-prefixed properties are internal. Never SELECT or RETURN them.
- Entity key is \`{name: "..."}\`, not \`{_id: "..."}\`.
- The Player is \`MATCH (p:Entity {name: "Player"})\`, never \`(p:Player)\`.
- ALL status/type values are UPPERCASE: 'CHARACTER', 'OBJECT', 'LOCATION', 'ACTIVE', 'PENDING', 'IN_PROGRESS', 'COMPLETED', 'ABANDONED'.
- This is Cypher, NOT SQL. Never use GROUP BY. Aggregation groups by non-aggregated RETURN columns automatically. Use \`COUNT(e)\` not \`COUNT(*)\`.
- For multiple status/type values use \`WHERE p.status IN ['ACTIVE', 'IN_PROGRESS']\` not \`{status: 'A', status: 'B'}\`.
- WHERE before RETURN. ORDER BY before LIMIT. RETURN columns comma-separated.
- In WHERE NOT, don't bind variables: \`WHERE NOT (e)-[:REL]->()\` not \`WHERE NOT (e)-[r:REL]->()\` or \`WHERE NOT (c:Label)-[:REL]->()\`.
- Never use \`|\` to alternate relationship types in patterns. Use multiple WHERE NOT clauses instead: \`WHERE NOT (e)-[:REL1]->() AND NOT (e)-[:REL2]->()\`.
- To find nodes with no relationships of a type: \`MATCH (l:Location) WHERE NOT EXISTS { (c:Character)-[:LOCATED_AT]->(l) } RETURN l\`.
- Never use map projection with pattern comprehension \`r [(n)-[r]->(m) | ...]\`. Just return \`TYPE(r)\` and the connected node names.
- To query Message history: \`MATCH (m:Message) RETURN m.role, m.content, m.timestamp ORDER BY m.timestamp DESC LIMIT 10\`.
- Notes have \`content\`, not \`contentSummary\` or \`summary\`. Plots use \`[:COMPLETED_AT]->(tp:TimePoint)\` not \`.completed_at\`.
- Entities have \`type\` (CHARACTER/OBJECT/LOCATION) and \`brief\` (one-line). Use \`e.type\` not \`TYPE(e)\` (TYPE is for relationships). Use \`e.brief\` for the short summary, \`e.description\` for the long text.

## OUTPUT

Respond with ONLY the raw Cypher query. No code fences. No explanation. No markdown. No prefix like "cypher". Just the query text, starting with MATCH.
`.trim();
}

// ── LLM helpers ──

function parseQueryResponse(raw: string): TranslateResult {
  let text = raw.trim();

  // Strip code fences
  text = text.replace(/^```(?:cypher|sql|graphql)?\s*\n?/i, "").replace(/\n?```\s*$/, "");

  // Strip leading "cypher" / "cypher\n" prefix the model sometimes emits
  text = text.replace(/^cypher\s*\n?/i, "");

  // Gemma model outputs literal \n escape sequences instead of real newlines
  text = text.replace(/\\n/g, "\n").replace(/\\t/g, "\t");

  text = text.trim();

  // Strip EXPLAIN prefix if the model emitted it (we add our own)
  text = text.replace(/^EXPLAIN\s+/i, "");

  // Fix duplicate LIMIT (e.g. LIMIT 10 LIMIT 50 → LIMIT 10)
  text = text.replace(/\bLIMIT\s+\d+\s+LIMIT\s+\d+\b/i, (m) => m.split(/\s+LIMIT\s+/i)[0]);

  // If the text starts with MATCH, CALL, or OPTIONAL MATCH, it's likely the query
  if (/^(MATCH|CALL|EXPLAIN|OPTIONAL|RETURN)\b/i.test(text)) {
    const explanation = text.split("\n").slice(-1)[0]?.trim() || "Query executed.";
    // Extract query: everything up to the last non-empty line that looks like Cypher
    const lines = text.split("\n");
    // Remove trailing explanation lines (those starting with lowercase, "This", "The", etc.)
    while (lines.length > 0 && /^(This|The|A|An|It|Returns|Finds|Lists|Shows|Retrieves|Matches)\b/.test(lines[lines.length - 1].trim())) {
      lines.pop();
    }
    return { query: lines.join("\n").trim(), explanation };
  }

  // Last resort: search for MATCH...RETURN pattern anywhere in the text
  const match = text.match(/(MATCH\s+[\s\S]*?RETURN\s+[\s\S]*?)(?:\n\n|\n\w|$)/i);
  if (match?.[1]) {
    return { query: match[1].trim(), explanation: "Query extracted from response." };
  }

  return { query: "", explanation: "" };
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
    console.error("[cypherTranslator] no query found in response, raw:", raw);
    throw new Error(`LLM did not return a recognizable Cypher query. Raw output:\n${raw.slice(0, 300)}`);
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

// ── Generate Cypher with retry ──

async function generateCypherWithRetries(
  llmUrl: string,
  systemPrompt: string,
  intent: string,
  maxRetries: number = 2,
): Promise<TranslateResult> {
  let lastError = "";
  let lastQuery = "";

  for (let attempt = 0; attempt <= maxRetries; attempt++) {
    const retryHint = attempt > 0
      ? `\n\nYour previous query failed:\n\`\`\`cypher\n${lastQuery}\n\`\`\`\nError: ${lastError}\nFix the query and try again.`
      : "";

    const { query, explanation } = await translateIntent(llmUrl, systemPrompt, intent + retryHint);
    lastQuery = query;

    // Validate
    const validator = new CypherValidator();
    const validation = validator.validateRead(query);
    if (!validation.valid) {
      lastError = validation.errors.join("; ");
      if (attempt < maxRetries) {
        console.error(`[cypherTranslator] validation failed (attempt ${attempt + 1}/${maxRetries + 1}), retrying:`, lastError);
        continue;
      }
      throw new Error(`Query failed validation after ${maxRetries + 1} attempts: ${lastError}`);
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
      lastError = explainErr instanceof Error ? explainErr.message : String(explainErr);
      if (attempt < maxRetries) {
        console.error(`[cypherTranslator] syntax error (attempt ${attempt + 1}/${maxRetries + 1}), retrying:`, lastError);
        continue;
      }
      throw new Error(`Generated query has syntax error after ${maxRetries + 1} attempts: ${lastError}`);
    }

    // Reached here = query is valid
    return { query: finalQuery, explanation };
  }

  throw new Error(`Failed after ${maxRetries + 1} attempts: ${lastError}`);
}

// ── Single-intent orchestrator ──

export async function executeCypherTranslator(
  llmUrl: string,
  intent: string,
  maxRetries: number = 2,
): Promise<CypherTranslatorResult> {
  const systemPrompt = await buildCypherTranslatorSystemPrompt();

  const { query, explanation } = await generateCypherWithRetries(llmUrl, systemPrompt, intent, maxRetries);

  // Execute
  const client = MemoryClient.getCachedInstance();
  const rows = await client.neo4j.executeRead(query);
  const safeRows = stripHiddenProperties(rows) as Record<string, unknown>[];

  // Round 2: format results as markdown (non-fatal if it fails)
  let markdown = "";
  try {
    markdown = await formatResult(llmUrl, intent, query, safeRows);
  } catch (formatErr) {
    const msg = formatErr instanceof Error ? formatErr.message : String(formatErr);
    console.error("[cypherTranslator] formatResult failed:", msg);
    markdown = `(Formatting failed: ${msg})`;
  }

  return {
    query,
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
  maxRetries: number = 2,
): Promise<CypherTranslatorBatchResult> {
  try {
    const { query, explanation } = await generateCypherWithRetries(llmUrl, systemPrompt, intent, maxRetries);

    const client = MemoryClient.getCachedInstance();
    const rows = await client.neo4j.executeRead(query);
    const safeRows = stripHiddenProperties(rows) as Record<string, unknown>[];

    let markdown = "";
    try {
      markdown = await formatResult(llmUrl, intent, query, safeRows);
    } catch (formatErr) {
      const msg = formatErr instanceof Error ? formatErr.message : String(formatErr);
      console.error("[cypherTranslator] formatResult failed for intent:", intent.slice(0, 80), msg);
      markdown = `(Formatting failed: ${msg})`;
    }

    return {
      intent,
      result: { query, explanation, markdown, rawRows: safeRows, rowCount: safeRows.length },
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
  maxRetries: number = 2,
): Promise<CypherTranslatorBatchResult[]> {
  const systemPrompt = await buildCypherTranslatorSystemPrompt();

  const results = await Promise.all(
    intents.map((intent) => processOneIntent(llmUrl, systemPrompt, intent, maxRetries)),
  );

  return results;
}

export { DEFAULT_LLM_URL };
