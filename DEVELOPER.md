# Developer Documentation: Elysian Dialogue

This document provides a comprehensive overview of the architecture, core systems, and data structures of the **Elysian Dialogue** application.

---

## 1. Project Overview

**Elysian Dialogue** is a cinematic RPG-style dialogue engine designed for immersive narrative experiences. It features a vertical-scrolling "thought stream" aesthetic, branching dialogue paths, and probabilistic skill checks influenced by character attributes.

- **Stack:** React 19, TypeScript, Vite.
- **Backend:** Express with SQLite (via `better-sqlite3`).
- **Intelligence:** Single-LLM Game Master (Gemini/DeepSeek) with streaming SSE.
- **Visuals:** Tailwind CSS (v4), `motion` for fluid animations, Lucide icons, and CodeMirror for debugging.

---

## 2. Project Structure

```text
.
├── .env.example             # Template for environment variables (GEMINI_API_KEY)
├── .gitignore
├── CHANGELOG.md             # Detailed history of codebase modifications
├── DEVELOPER.md             # Technical documentation (this file)
├── ISSUES.md                # Known issues and bug tracking
├── game.db                  # SQLite database (World state, Narrative history, Dialogue tree)
├── index.html               # Frontend entry point
├── package.json             # Manifest with scripts and dependencies
├── tsconfig.json            # TypeScript build configuration
├── vite.config.ts           # Vite development and build configuration
└── src/
    ├── client/              # Frontend (React)
    │   ├── main.tsx         # App entry point
    │   ├── App.tsx          # Main orchestrator with SSE streaming consumer
    │   └── index.css        # Global styles (Tailwind + Custom noise filters)
    ├── components/          # UI Components
    │   ├── CharacterPanel.tsx   # Sidebar for stats and entity tracker
    │   ├── DebugPanel.tsx       # Developer toolbox (SSE logs, DB editors, Console)
    │   ├── DialogueMessage.tsx  # Message rendering (supports speaker types & links)
    │   ├── DialogueOptions.tsx  # Player choices (Actions, Checks, Continue)
    │   ├── DiceRoller.tsx       # Skill check simulation logic
    │   ├── ObjectLink.tsx       # Hoverable text references in world
    │   ├── ObjectTooltip.tsx    # Detailed entity lore popups
    │   └── TypingIndicator.tsx  # NPC activity feedback
    ├── context/             # React Contexts
    │   └── CharacterContext.tsx # Global character attribute and stat state
    ├── data/                # Static Data
    │   └── sampleDialogue.ts    # Initial scenario/tutorial steps
    ├── server/              # Backend (Node.js/Express)
    │   ├── main.ts          # Server entry and Vite middleware setup
    │   ├── api.ts           # REST API + SSE streaming endpoints
    │   ├── db.ts            # SQLite connection and schema definitions
    │   ├── LlmServiceBackend.ts # Single-LLM streaming GM logic (Prompting, Tools)
    │   ├── LlmDebugIntegration.ts # LLM request/response/step logging
    │   ├── sseEvents.ts     # SSE event emitter and narrative text parser
    │   └── models/          # Database Abstractions
    │       ├── debug.ts     # Logging queries (TRACES, CONSOLE)
    │       ├── dialogue.ts  # Dialogue tree CRUD (steps, alternatives, branches)
    │       ├── history.ts   # Narrative flow persistence (Message storage)
    │       ├── plot.ts      # Objective tracking
    │       └── world.ts     # Entity state management (Characters, Objects, Locations)
    ├── services/            # Shared Logic
    │   ├── ConsoleLogger.ts # System-wide console log interception
    │   ├── LlmService.ts    # Frontend AI client (deprecated, used as mock)
    │   ├── SseClient.ts     # Browser SSE streaming consumer & event dispatcher
    │   ├── WorldManager.ts  # World state synchronization client
    │   └── tools/           # LLM Tool Implementations (Vercel AI SDK compatible)
    │       ├── updateWorldState.ts      # Commit entity changes directly
    │       ├── updatePlotStatus.ts      # Commit plot status changes
    │       └── createPlot.ts           # Create new plot/quest
    └── types/               # TypeScript Definitions
        ├── dialogue.ts      # Narrative system interfaces (Message, Option, Step)
        └── entities.ts      # Character and actor models (Stats, Opinions)
```

---

## 3. Architecture Overview

### 3.1 Event-Driven Streaming

The application uses Server-Sent Events (SSE) to bridge the LLM's streaming output to the React UI.

**Lifecycle of a Turn:**
1.  **Request:** Frontend sends `POST /api/chat/stream`.
2.  **Streaming:** `LlmServiceBackend` uses `streamText` from `@ai-sdk`.
3.  **Event Dispatch:** `TurnEventEmitter` pipes tokens and tool results to the client.
    - `token`: Raw text chunks.
    - `world_update` / `plot_update`: Emitted when tools execute.
    - `options`: Emitted when the GM completes the `<OPTIONS>` block.
    - `parsed`: Emitted after full turn completion with structured data.
4.  **Consuming:** `SseClient` on the frontend updates local state (typing, messages).

### 3.2 Automated State Management

The Game Master is "State-Aware". It receives the full World State and Active Plots in its system prompt. Tools allow it to:
- **Update Entities:** Change opinions of NPCs on the fly based on player choices.
- **Update Plots:** Move quests forward without developer intervention.
- **Create Content:** Generate new plots if the player goes off-script.

### 3.3 Structured Dialogue Generation

Narrative output is generated via the `generateDialogueStep` tool, which enforces the `DialogueStep` schema. This ensures perfect consistency and type safety.
- **Messages:** A sequence of structured message objects (speaker, type, text).
- **Options:** Player choices, including hints and skill check conditions.
- **Streaming:** The server partially parses the tool's JSON arguments to maintain a "Thought Stream" effect on the frontend.

### 3.4 Dialogue Branching & Alternatives

- **Steps:** A single interaction "moment".
- **Branches:** When a user selects an option, a new child step is created.
- **Alternatives:** When a user clicks "Regenerate", the current step is archived as an alternative, and a new one is generated. The UI allows "swiping" between these versions.

### 3.5 Pre-Generation (Asynchronous)

After generating a step, the backend asynchronously pre-generates the next step for each `isAiTrigger` option. These are stored as child nodes. When a user clicks an option, the frontend first checks for a pre-generated child — if found, it loads instantly without an API call, significantly reducing perceived latency.

---

## 4. API Endpoints

### 4.1 Chat & AI
- `POST /api/chat/stream`: The primary entry point for AI turns.
- `POST /api/regenerate`: Triggers a new generation for an existing step ID.

### 4.2 Dialogue Tree
- `GET /api/dialogue/:id`: Returns step data, including child options and alternative versions.
- `GET /api/dialogue/:id/path`: Returns the full sequence of steps from root to the specified node.

### 4.3 State & Debug
- `GET /api/world`: Fetch all known entities.
- `GET /api/plots`: Fetch all active and resolved plots.
- `GET /api/debug/traces`: Fetch LLM interaction logs.
- `GET /api/debug/logs`: Fetch intercepted console logs.

---

## 5. Core Systems

### 5.1 Internal Voices (Ego Skills)

The game implements a Disco Elysium-style internal monologue. Recognized voices include:
`LOGIC`, `RHETORIC`, `EMPATHY`, `PERCEPTION`, `VOLITION`, `ENDURANCE`, `INLAND EMPIRE`, `SUGGESTION`, `HALF LIGHT`, `PHYSICAL INSTRUMENT`, `INTERFACING`, `ELECTROCHEMISTRY`.

These map to **Character Stats** in `src/types/entities.ts` and are used for skill checks in `DiceRoller.tsx`.

### 5.2 Skill Checks

- **White Checks:** Repeatable after stat increases.
- **Red Checks:** High-stakes, one-time opportunities (indicated by `isRed` in `DialogueOption`).
- **Logic:** `2d6 + Stat >= Difficulty`.

### 5.3 Debug Panel Features

The Debug Panel (`/components/DebugPanel.tsx`) is a dual-purpose tool for designers and developers:
- **World Editor:** Live-edit any entity's attributes or descriptions.
- **History Editor:** Rewrite recent conversation history to steer the AI.
- **Trace Viewer:** Inspect raw JSON payloads, tool arguments, and total token usage per step.

---

## 6. Development Workflow

### 6.1 Adding a New Voice/Skill
1.  Update `CharacterStats` in `src/types/entities.ts`.
2.  Update `VOICE_NAMES` in `src/server/sseEvents.ts`.
3.  Add entry to the system prompt in `src/server/LlmServiceBackend.ts`.
4.  Add default value in `src/context/CharacterContext.tsx`.

### 6.2 Managing the World
Initial world state is seeded in `src/server/models/world.ts`. To change starting locations or characters, modify the `entities` collection there.

### 6.3 Building & Running
- `npm run dev`: Starts the Express server with Vite middleware.
- `npm run build`: Bundles the React client and prepares the server for production.
- `npm run start`: Runs the production-ready server.
