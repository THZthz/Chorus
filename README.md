# Elysian Dialogue

> WARNING: Very early in development stage. The story-telling experience is poor right now!

Cinematic RPG-style dialogue engine with a fantasy-steampunk setting. The AI Game Master generates branching narrative through tool-calling, streamed to a console client in real-time via SSE. Player choices are guided by twelve inner voices — each a distinct personality mapped to a character stat — with skill checks resolved through 2D6 dice rolls.

## Tech Stack

| Layer     | Technology                                  |
|-----------|---------------------------------------------|
| Console   | TypeScript, Node.js, chalk (`@inquirer/prompts`) |
| Backend   | Express, SQLite (`better-sqlite3`), Neo4j   |
| AI        | Gemini / DeepSeek via Vercel AI SDK v6      |
| Streaming | Server-Sent Events                          |
| Memory    | Neo4j via `agent-memory` MCP server (16 tools) |

## Getting Started

### Prerequisites

- Node.js 20+
- Python 3.10+ (for agent-memory MCP server)
- Neo4j 5.x running locally (`bolt://localhost:7687`)
- A Gemini or DeepSeek API key

### Setup

```bash
cp .env.example .env
# Add your API key to .env:
#   GEMINI_API_KEY=your_key_here
#   DEEPSEEK_API_KEY=your_key_here
#   NEO4J_PASSWORD=your_neo4j_password

npm install

# Terminal 1 — Start the agent-memory MCP server
cd reference/agent-memory
uv run python run_elysian_mcp.py --password $NEO4J_PASSWORD

# Terminal 2 — Start Elysian Dialogue
NEO4J_PASSWORD=your_password npm start

# Terminal 3 — Play
npm run console
```

On first run, Neo4j is seeded with a default fantasy-steampunk world — characters, locations, objects, and a root plot with three branching threads.

## How It Works

1. **Player chooses an action** — dialogue option, skill check, or custom input
2. **AI Game Master responds** — the LLM uses MCP tools to query Neo4j (entities, relationships, facts), then generates narrative
3. **Streaming to console** — typed SSE events deliver progressive messages in real-time
4. **Inner voices chime in** — twelve skills (Logic, Rhetoric, Empathy, Perception, Volition, Endurance, Sorcery, Suggestion, Instinct, Might, Clockwork, Alchemy) each have distinct personalities that comment on the situation

### World Memory

All game state lives in Neo4j: characters, locations, objects, factions, plots, relationships, facts, and conversation history. The GM accesses it through 16 MCP tools auto-discovered from the `agent-memory` MCP server — semantic search, graph traversal, entity CRUD, relationship management, and Cypher queries.

### Skill Checks

White checks (repeatable) and red checks (one-time, high-stakes). Formula: `2d6 + Stat >= Difficulty`. Probability is displayed before rolling. Natural 2 and 12 have distinct outcomes.

## Developer Documentation

See [DEVELOPER.md](DEVELOPER.md) for architecture details, API endpoints, database schema, event system, and workflow guides.

## License

[AGPL v3](./LICENSE)
