import { MemoryClient } from "@/server/memory/client";
import { CypherValidator } from "@/server/memory/validation";
import { stripHiddenProperties } from "@/server/memory/neo4j";
import { getSchemaVisualization, getRelationshipTypeDescriptions, getNodeProperties, formatSchemaMarkdown } from "@/server/models/schema";

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

  const [schemaVis, relTypeDescs, nodeProps] = await Promise.all([
    getSchemaVisualization(db),
    getRelationshipTypeDescriptions(db),
    getNodeProperties(db),
  ]);

  // Node labels + indexes from schema; relationship types handled by buildDynamicRules
  const schemaSection = formatSchemaMarkdown(schemaVis, relTypeDescs)
    .replace(/\n### Relationship Types\n[\s\S]*/, ""); // strip duplicate — buildDynamicRules has descriptions
  const dynamicRules = buildDynamicRules(schemaVis, relTypeDescs, nodeProps);

  return `
You are a precise Cypher query generator for a Neo4j-backed RPG engine.
Given a natural-language intent, produce a read-only Cypher query.

${schemaSection}

${dynamicRules}

Respond with ONLY the raw Cypher query. No code fences. No explanation. No markdown. No prefix like "cypher". Just the query text, starting with MATCH.
`.trim();
}

// ── Dynamic rule builder (adapts to live schema) ──

function buildDynamicRules(
  schemaVis: import("@/server/models/schema").SchemaVisualization,
  relTypeDescs: import("@/server/models/schema").RelationshipTypeDescription[],
  nodeProps: Map<string, string[]>,
): string {
  const parts: string[] = [];

  // Relationship types from live :RelationshipType nodes
  const descByName = new Map(relTypeDescs.map((d) => [d.name, d]));
  const seenTypes = new Set<string>();

  parts.push("## RELATIONSHIP TYPES");
  for (const rel of schemaVis.relationships) {
    if (seenTypes.has(rel.type)) continue;
    seenTypes.add(rel.type);
    const desc = descByName.get(rel.type);
    const src = rel.sourceLabels?.join("/") || "?";
    const tgt = rel.targetLabels?.join("/") || "?";
    if (rel.type.startsWith("_")) {
      parts.push(`- **${rel.type}** (${src})→(${tgt}) — INTERNAL, never query`);
    } else if (desc) {
      parts.push(`- **${rel.type}** (${src})→(${tgt}) (${desc.category}): ${desc.description}`);
    } else {
      parts.push(`- **${rel.type}** (${src})→(${tgt})`);
    }
  }
  parts.push("");

  // Collect Entity dynamic sub-labels from schema
  const nodeLabels = new Set<string>();
  for (const node of schemaVis.nodes) {
    for (const label of node.labels) nodeLabels.add(label);
  }
  const entitySubs = ["Character", "Object", "Location", "Organization", "Event"]
    .filter((l) => nodeLabels.has(l));

  // Node-specific notes
  parts.push("## NODE PROPERTIES");
  const sortedLabels = [...nodeLabels].filter((l) =>
    !l.startsWith("_") && l !== "IdCounter" && l !== "Conversation" && l !== "GMTurnMessage"
  ).sort();
  for (const label of sortedLabels) {
    const props = nodeProps.get(label);
    const propList = props?.length ? props.filter((p) => !p.startsWith("_")).join(", ") : "(none)";
    const hidden = props?.filter((p) => p.startsWith("_")).join(", ") || "";
    let line = `- **${label}**: ${propList}`;
    if (hidden) line += ` (internal: ${hidden})`;
    if (entitySubs.includes(label)) line += " — dynamic Entity sub-label, inherits all Entity properties above";
    parts.push(line);
  }
  if (nodeLabels.has("NPCDisposition")) {
    // Replace generic NPCDisposition line with more detailed note
    const idx = parts.findIndex((l) => l.startsWith("- **NPCDisposition**"));
    if (idx !== -1) {
      parts[idx] += ' — **NODE, not a relationship**. Match dispositions with `(npc:Entity)-[:HAS_DISPOSITION]->(d:NPCDisposition {target_name: "Player"})`.';
    }
  }
  parts.push('- The Player is `MATCH (p:Entity {name: "Player"})`.');
  parts.push("");

  // Universal Cypher rules
  parts.push("## RULES");
  parts.push("- Read-only ONLY (MATCH, RETURN, ORDER BY, LIMIT, WHERE, WITH, OPTIONAL MATCH, COLLECT).");
  parts.push("- Use ONLY labels, properties, relationships from the Schema above.");
  parts.push("- Use `COLLECT { }` for lists. Use `OPTIONAL MATCH` only for single optional links.");
  parts.push("- Never unbounded variable-length paths. Use a fixed upper bound like `[*1..5]`.");
  parts.push("- `_`-prefixed properties are internal. Never SELECT or RETURN them.");
  parts.push('- Entity key is `{name: "..."}`, not `{_id: "..."}`.');
  parts.push("- ALL status/type values are UPPERCASE: CHARACTER, OBJECT, LOCATION, ACTIVE, PENDING, IN_PROGRESS, COMPLETED, ABANDONED.");
  parts.push("- Cypher, NOT SQL. Never use GROUP BY. Use `COUNT(e)` not `COUNT(*)`.");
  parts.push("- For multiple values: `WHERE p.status IN ['ACTIVE', 'IN_PROGRESS']` not `{status: 'A', status: 'B'}`.");
  parts.push("- WHERE before RETURN. ORDER BY before LIMIT.");
  parts.push("- In WHERE NOT, don't bind variables: `WHERE NOT (e)-[:REL]->()` not `WHERE NOT (e)-[r:REL]->()`.");
  parts.push("- Never use `|` in patterns. Use multiple AND NOT clauses.");
  parts.push("- Anti-pattern: `MATCH (l:Location) WHERE NOT EXISTS { (c:Character)-[:LOCATED_AT]->(l) } RETURN l`.");
  parts.push("- Message history: `MATCH (m:Message) RETURN m.role, m.content, m.timestamp ORDER BY m.timestamp DESC LIMIT 10`.");
  parts.push("- Notes: `content` not `contentSummary`. Plots: `[:COMPLETED_AT]->(tp)` not `.completed_at`.");
  parts.push("- Entities: `e.type` not `TYPE(e)`. `e.brief` for summary, `e.description` for full text.");
  parts.push("- Isolated entities: list relationship types explicitly. Never `[:*]`.");
  parts.push("");

  return parts.join("\n");
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
    let retryGuidance = "";
    if (attempt > 0) {
      retryGuidance = `\n\nYour previous query failed:\n\`\`\`cypher\n${lastQuery}\n\`\`\`\nError: ${lastError}`;
      if (/\[:\*\]/.test(lastQuery)) {
        retryGuidance += `\n\n[:*] is invalid Cypher. To find nodes with no relationships, list types explicitly:\n\`\`\`cypher\nWHERE NOT (e)-[:LOCATED_AT]->() AND NOT (e)-[:CARRIES]->() AND NOT (e)-[:ALLIED_WITH]->() AND NOT (e)-[:HOSTILE_TOWARDS]->() AND NOT (e)-[:CONNECTED_TO]->() AND NOT (e)-[:HAS_DISPOSITION]->()\n\`\`\``;
      }
      retryGuidance += "\nFix the query and try again.";
    }

    const { query, explanation } = await translateIntent(llmUrl, systemPrompt, intent + retryGuidance);
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
      // Inject guidance for [: *] pattern
      if (/\[:\*\]/.test(finalQuery)) {
        lastError += "\n[:*] is invalid Cypher. List relationship types explicitly: WHERE NOT (e)-[:LOCATED_AT]->() AND NOT (e)-[:CARRIES]->() ...";
      }
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
