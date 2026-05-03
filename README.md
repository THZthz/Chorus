# Elysian Dialogue

Cinematic RPG-style dialogue engine with a fantasy-steampunk setting. The AI Game Master generates branching narrative through tool-calling, streamed to the frontend in real-time via SSE. Player choices are guided by twelve inner voices — each a distinct personality mapped to a character stat — with skill checks resolved through 2D6 dice rolls.

## Tech Stack

| Layer     | Technology                                  |
|-----------|---------------------------------------------|
| Frontend  | React 19, TypeScript, Vite                  |
| Backend   | Express, SQLite (`better-sqlite3`)          |
| AI        | Gemini / DeepSeek via Vercel AI SDK v6      |
| Styling   | Tailwind CSS v4, `motion`, Lucide icons     |
| Streaming | Server-Sent Events                          |

## Getting Started

### Prerequisites

- Node.js 20+
- A Gemini or DeepSeek API key

### Setup

```bash
cp .env.example .env
# Add your API key to .env:
#   GEMINI_API_KEY=your_key_here
#   DEEPSEEK_API_KEY=your_key_here

npm install
npm run dev
```

The server starts at `http://localhost:3000`. On first run, the database is seeded with a default fantasy-steampunk world — characters, locations, objects, and a root plot with two branching threads.

### Build (production)

```bash
npm run build
npm start
```

## How It Works

1. **Player chooses an action** — dialogue option or skill check
2. **AI Game Master responds** — the LLM uses tool calls to query entities, create/update plots, mutate the world, and generate narrative
3. **Streaming to frontend** — typed SSE events deliver progressive messages, world mutations, and plot updates in real-time
4. **Inner voices chime in** — twelve skills (Logic, Rhetoric, Empathy, Perception, Volition, Endurance, Sorcery, Suggestion, Instinct, Might, Clockwork, Alchemy) each have distinct personalities that comment on the situation

### Skill Checks

White checks (repeatable) and red checks (one-shot) are resolved with **2D6 + stat vs difficulty**. The DiceRoller component shows success probability, animates the roll, and feeds the result back into the narrative.

### Dialogue Branching

Every choice creates a new step in the dialogue tree, linked bidirectionally to its parent. Regenerating a step archives the old version as an alternative — you can swipe between versions. Replay mode lets you navigate the full tree, jump to any node, and spawn new branches from unexplored options.

### Plots

The story is structured as a plot tree — the GM creates and edits plots first, then generates dialogue options that map to active plot branches. This keeps the narrative coherent across long sessions.

## Project Structure

```
src/
├── client/          # React entry point, state machine, SSE consumer
├── components/      # UI components (dialogue, panels, dice roller)
│   └── debug/       # Debug panel tabs (LLM trace, world editor, graphs)
├── context/         # React context (character stats)
├── server/
│   ├── llm/         # Game Master: prompts, tools, SSE events, debug logging
│   └── models/      # Database CRUD (entities, dialogue, plots, history)
├── services/        # Client-side SSE client, console logger, world manager
├── shared/          # Shared SSE event type definitions
└── types/           # TypeScript interfaces (dialogue, entities, plots)
```

## Developer Documentation

See [DEVELOPER.md](DEVELOPER.md) for architecture deep-dive, API endpoints, database schema, event system, and workflow guides.

## License

Private
