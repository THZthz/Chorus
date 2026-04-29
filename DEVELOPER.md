# Developer Documentation: Elysian Dialogue

This document provides a comprehensive overview of the architecture, core systems, and data structures of the **Elysian Dialogue** application.

---

## 1. Project Overview

**Elysian Dialogue** is a cinematic RPG-style dialogue engine designed for immersive narrative experiences. It features a vertical-scrolling "thought stream" aesthetic, branching dialogue paths, and probabilistic skill checks influenced by character attributes.

- **Stack:** React 19, TypeScript, Vite.
- **Backend:** Express with SQLite (via `better-sqlite3`).
- **Intelligence:** Gemini-powered autonomous DM using tool calling.
- **Visuals:** Tailwind CSS (v4), `motion` for fluid animations, and Lucide icons.

---

## 2. Project Structure

The project follows a full-stack monorepo-style structure where the backend serves both the API and the frontend assets in production.

```text
.
├── .env.example             # Template for environment variables (GEMINI_API_KEY)
├── .gitignore               # Standard git exclusions
├── CHANGELOG.md             # Detailed history of codebase modifications
├── DEVELOPER.md             # Technical documentation (this file)
├── game.db                  # SQLite database (World state, Narrative history)
├── game.db-shm / -wal       # SQLite temporary/journal files
├── index.html               # Frontend entry point
├── metadata.json            # AI Studio application metadata
├── package.json             # Manifest with scripts and dependencies
├── tsconfig.json            # TypeScript build configuration
├── vite.config.ts           # Vite development and build configuration
└── src/
    ├── client/              # Frontend (React)
    │   ├── main.tsx         # App entry point
    │   ├── App.tsx          # Main orchestrator and message loop
    │   └── index.css        # Global styles (Tailwind + Custom filters)
    ├── components/          # UI Components
    │   ├── CharacterPanel.tsx   # Sidebar for stats and inventory
    │   ├── DebugPanel.tsx       # Developer toolbox (Logs, Editors)
    │   ├── DialogueMessage.tsx  # Message rendering and tooltip integration
    │   ├── DialogueOptions.tsx  # Player interaction buttons
    │   ├── DiceRoller.tsx       # Skill check animation and logic
    │   ├── ObjectLink.tsx       # Hoverable text references
    │   ├── ObjectTooltip.tsx    # Detailed entity lore popups
    │   └── TypingIndicator.tsx  # NPC activity feedback
    ├── context/             # React Contexts
    │   └── CharacterContext.tsx # Global character attribute state
    ├── data/                # Static Data
    │   └── sampleDialogue.ts    # Initial scenario/tutorial steps
    ├── server/              # Backend (Node.js/Express)
    │   ├── main.ts          # Server entry and Vite middleware setup
    │   ├── api.ts           # REST API route definitions
    │   ├── database.ts      # SQLite connection and schema migrations
    │   ├── LlmServiceBackend.ts # AI agent logic and configurations
    │   └── models/          # Database Abstractions
    │       ├── debug.ts     # Logging queries
    │       ├── history.ts   # Narrative flow persistence
    │       ├── plot.ts      # Objective tracking
    │       └── world.ts     # Entity state management
    ├── services/            # Shared Logic
    │   ├── ConsoleLogger.ts # System-wide log interception
    │   ├── LlmService.ts    # Frontend AI client API
    │   ├── WorldManager.ts  # World state synchronization client
    │   └── tools/           # LLM Tool Implementations
    │       ├── commitDrafts.ts         # Assistant tool to finalize and execute GM proposals
    │       ├── communicateAssistant.ts # GM tool to submit drafted state changes for review
    │       ├── draftDialogueStep.ts    # GM tool to propose next narrative unit
    │       ├── draftPlot.ts            # GM tool to propose a quest/plot line
    │       ├── draftPlotStatusUpdate.ts # GM tool to propose quest progress
    │       ├── draftWorldStateUpdate.ts # GM tool to propose entity attribute changes
    │       └── replyToGM.ts            # Assistant tool to request revisions from GM
    └── types/               # TypeScript Definitions
        ├── dialogue.ts      # Narrative system interfaces
        └── entities.ts      # Character and actor models
```

---

## 3. Architecture Overview

### 3.1 Frontend Orchestration
The root `App.tsx` manages the **Sequential Message Loop**. It handles the timing of incoming narrative blocks, interrupts for player input, and states for "Fast Forwarding" text.

### 3.2 Backend & Data Persistence
- **Express Server**: Acts as a proxy for LLM requests (to secure API keys) and provides a persistence layer.
- **SQLite**: Stores the world state, conversation history, and debugging logs. The schema is initialized in `src/server/database.ts`.

### 3.3 Communication Flow
1. **Frontend** captures user input or triggers a transition.
2. **`LlmService`** sends context (History + World + Plots) to `/api/chat`.
3. **Backend** invokes Gemini, which may call **Tools** to update the database.
4. **Backend** returns a new `DialogueStep`.
5. **Frontend** renders the new step sequentially.

---

## 4. Gameplay Systems

### 4.1 Dialogue Execution Loop
Each narrative unit is a `DialogueStep`.
- **Messages**: Displayed one-by-one with dynamic delays based on readability.
- **Options**: Displayed once messages finish. Can lead to other steps or trigger checks.

### 4.2 Skill Check Mechanics
Checks are defined in `DialogueOption`.
- **Logic**: Handled by `DiceRoller.tsx`.
- **Evaluator**: Uses `new Function()` in `App.tsx` to safely determine outcomes based on dice rolls and bonuses provided by `CharacterContext`.

### 4.3 Interactive World (Object Links)
Messages support `[Display Name](#entity_id)` syntax.
- **Hover**: Opens `ObjectTooltip` showing lore and status.
- **Bridge**: A hidden padding "bridge" allows moving the mouse from the trigger link to the tooltip without closing it.

---

## 5. AI Integration (Autonomous Game Master & Assistant)

The app utilizes a dual-LLM architecture (Powered by Vercel AI SDK and Gemini) to ensure high-quality and rule-abiding state transitions.

### 5.1 System Context
The agents receive:
- **Narrative History**: Recent messages to maintain tone.
- **World State**: Current attributes and locations of all actors/objects.
- **Plots**: Current active objectives.

### 5.2 Tooling & Drafting Loop
The GM model cannot directly modify the game world. Instead, it proposes drafts:
- `draftUpdateWorldState`: Propose changes to entity properties.
- `draftAddPlot`/`draftUpdatePlotStatus`: Propose dynamic quest changes.
- `draftAddDialogueStep`: Propose the user's next narrative response and choices.
- `communicateAssistant`: The GM must submit these drafts to the **Assistant LLM**.

The Assistant LLM evaluates the GM's drafts:
- `commitDrafts`: Commits the proposed changes if they are narratively consistent and syntactically correct (e.g. including `isAiTrigger`).
- `replyToGM`: Rejects the drafts and provides reasoning for the GM to revise them.

This creates an internal multi-step critique loop until a valid state transition is produced.

---

## 6. Development & Debugging

### 6.1 Debug Panel
A multi-layered utility reachable via the "Debug" overlay.
- **Logs**: Recharts LLM trace payloads with interactive JSON viewing and optional text wrapping.
- **Console**: Displays intercepted browser logs (piped via `ConsoleLogger`) with filtering and text wrapping.
- **State Editors**: CodeMirror instances that allow direct "Live" editing of World State or History with JSON Schema validation.

### 6.2 Logging Infrastructure
All LLM requests and console activities are persisted in SQLite to allow "Time-Travel" debugging or session restoration. `ConsoleLogger.ts` specifically handles `Error` object serialization to ensure stack traces and messages are not lost during JSON stringification (fixing the `{}` error issue). LLM logs are initiated at the start of the request to capture failures.

---

## 7. How to Extend

### 7.1 Adding New Content
- **Initial Seeds**: Modify `src/data/sampleDialogue.ts`.
- **Character/Objects**: Update the initial state in `src/services/WorldManager.ts` or use the **World Editor** in the Debug Panel.
- **Skills**: Add to `CharacterStats` in `src/types/entities.ts` and initialize in `CharacterContext.tsx`.

---

## 8. AI / LLM Agent Instructions

If you are an AI developer working on this project:
1. **Maintain this Doc**: Update the project tree and descriptions when adding features.
2. **Context First**: Always read `DEVELOPER.md` and `CHANGELOG.md` before refactoring core systems.
3. **Typing**: Ensure new types are shared across the full stack in `src/types`.

> **Instruction for AI / LLM Agents:** If you are an LLM agent developing, extending, or refactoring this project, you must keep this `DEVELOPER.md` document up to date. Revise architectural outlines, descriptions, data structures, and the AI integration behaviors here when you implement underlying changes!