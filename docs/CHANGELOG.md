# Changelog: Elysian Dialogue

All notable changes to this project will be documented in this file. Should be extremely concise for each entry. Only main change can be added here.

### 2026-04-30
- **AI Engine**: Fully transitioned to structured dialogue generation via the `dialogue_response` tool. Removed all raw text parsing fallbacks (`parseResponseText`). Enhanced tool schema to support skill checks, hints, and custom metadata. Simplified streaming loop in `LlmServiceBackend.ts` to strictly handle tool-delta updates.
- **Architecture**: Major refactoring to event-driven, streaming-first architecture. Removed the dual-LLM (GM + Assistant) verification loop. Single GM LLM commits directly via tools. SSE streaming for real-time text display. Dialogue tree persisted in SQLite with pre-generation and regenerate support.
- **New Tables**: `dialogue_steps` (tree structure) and `dialogue_alternatives` (regenerate/swipe support).
- **New Endpoints**: `/api/chat/stream` (SSE), `/api/dialogue/:id` (step tree), `/api/regenerate` (regenerate via SSE), `/api/branches/activate` (branch switching).
- **New Files**: `src/server/sseEvents.ts`, `src/server/models/dialogue.ts`, `src/services/SseClient.ts`, `src/services/tools/updateWorldState.ts`, `src/services/tools/updatePlotStatus.ts`, `src/services/tools/createPlot.ts`.
- **Removed**: All Assistant-related files (`communicateAssistant.ts`, `commitDrafts.ts`, `replyToGM.ts`) and draft tools (`draftWorldStateUpdate.ts`, `draftDialogueStep.ts`, `draftPlot.ts`, `draftPlotStatusUpdate.ts`).
- **Frontend**: `App.tsx` now consumes SSE streaming. Added regenerate button. `DialogueMessage.tsx` supports `isStreaming` prop with blinking cursor.

### 2026-04-29
- **Debug Panel**: Simplified the `GM_Assistant_Communication` and request/response blocks in the LLM trace by removing `ResizableContainer` handles and excessive UI clutter, optimizing for information density.
- **Build Fix**: Resolved server-side build failures by moving all server and tool imports to relative paths (fixing esbuild resolution issues with the `@/` alias).
- **Build Fix**: Fixed mismatched import/export names for GM drafting tools in `LlmServiceBackend.ts`.
- **Build Fix**: Updated `LlmServiceBackend.ts` to reflect the renaming of `draftUpdateWorldState.ts` to `draftWorldStateUpdate.ts`.
- **Docs**: Updated `DEVELOPER.md` section 2 "Project Structure" to reflect actual file names and the recent rename.

### 2026-04-28
- **Bug Fix / Refactoring**: Addressed user-reported `UNIQUE constraint failed: history_messages.id`. Reverted previous `INSERT OR IGNORE` suppression. Transferred AI message persistence logic entirely to the `commitDrafts` tool. Added `id` requirement to `messages` schema in `addDialogueStep` to let the GM deliberately generate unique identifiers. Unique constraint errors and other DB exceptions are now caught securely within `commitDrafts` and passed directly as text to the Assistant LLM, satisfying the user request "feed the error back to llm to let it fix it". 
- **Bug Fix**: Added `try-catch` when inserting the player's message in `/api/chat` to safely ignore duplicate insertions (caused by UI `isContinue` loops sending identical state) without crashing the node server or LLM service.
- **Bug Fix**: Resolved `Invalid schema` (type: null) error for `communicateAssistant` tool by migrating all tool definitions from `parameters` to `inputSchema`, adhering to AI SDK 5.0+ standards.
- **AI Engine**: Updated multi-step loop control from `maxSteps` to newer `stopWhen: stepCountIs(N)` API.
- **AI Engine**: Hardened tool schemas with required descriptions and avoided empty parameter objects for better provider compatibility (especially DeepSeek).
- **Refactoring**: Removed placeholder `execute` functions from base tools to resolve TypeScript type override conflicts in factory-generated tools.
- **Refactoring**: Redefined model aliases in `LlmServiceBackend.ts` to use standard, stable identifiers.
- **Refactoring**: Modularized agent tools. Moved inline GM drafting and Assistant verification tools from `LlmServiceBackend.ts` to dedicated files in `src/services/tools/` using factory patterns for dependency injection (e.g., drafts, callbacks).
- **Debug Panel**: Added "WRAP" button to Console logs, consistent with LLM trace functionality.
- **AI Engine**: Added a double-LLM verification loop in `LlmServiceBackend.ts`. The primary "GM" can only draft changes to the world state, plots, and dialogue using new draft-based tools. A secondary "Assistant" LLM reviews the GM's drafts and forces revisions or commits them if they are sound, ensuring stricter adherence to rules.
- **Bug Fix**: Corrected `Error` object serialization in `ConsoleLogger.ts`. Previously, errors were logged as empty objects `{}` due to non-enumerable properties; improved with custom Zod-friendly serialization and stack trace capture.
- **Logging**: LLM interaction logs are now initiated at the start of the request, ensuring visibility even during catastrophic API failures.

### 2026-04-27
- **Docs**: Restructured `DEVELOPER.md` for better organization and updated the project structure tree.
- **Docs**: Summarized AI capabilities in `DEVELOPER.md` including context, logic loop, and tools for another LLM instructions.
- **Debug Panel**: Added comprehensive filtering for Console logs (Keywords/Regex, Level toggles, Date Range).
- **Debug Panel**: Implemented SQLite persistence for console logs. Captures browser logs in real-time and hydrates the view on refresh.
- **Debug Panel**: Added date to timestamps in LLM trace and Console logs for better troubleshooting.
- **Debug Panel**: Added text wrap toggle for LLM logs and fixed related horizontal layout issues.
- **Debug Panel**: Enhanced `ConsoleViewer` and LLM trace with interactive JSON inspection and visual fixes.
- **Project**: Reorganized codebase into `client/` and `server/` and updated documentation.
- **UI**: Polished debug panel interaction and simplified changelog format.

### 2026-04-25
- **Advanced Debugging**: Major upgrade to the `DebugPanel`.
- **Design & UI**: Unified toggle button aesthetics and introduced "One Dark" syntax highlighting.
- **System**: Refined LLM request logging and corrected model configurations.

### 2026-04-24
- **Layout**: Resolved critical scroll container and viewport height issues.

### 2026-04-23
- **AI Engine**: Migrated to **Vercel AI SDK** with **Zod** schema validation.
- **Narrative**: Switched core model to **DeepSeek-V3** and hardened system prompts.
- **Polish**: Applied global cinematic styling and refined message flow animations.
