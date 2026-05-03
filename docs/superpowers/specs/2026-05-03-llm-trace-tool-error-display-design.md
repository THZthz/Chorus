# LLM Trace: Tool Call Error Display in PARSED_EXCHANGE

## Context

The LLM Trace Viewer's PARSED_EXCHANGE section shows each tool call and its result. When a tool call fails, the error should always be visible, regardless of failure type.

## Failure scenarios

1. **`wrapSafe` catches exception** — tool returns `"ERROR: ..."` string. Currently shown in red, but not visually distinct from normal results.
2. **No result paired** — `call.output` is null/undefined. Currently the entire "Result" section is hidden.
3. **SDK-level `tool-error`** — AI SDK emits `tool-error` type with `error` property. Not captured in current tool_results.

## Design

Single file change: `src/components/debug/LlmTraceViewer.tsx`

### 1. Always show result section

Remove `call.output != null` guard. Every tool call shows a result area.

### 2. Three result states

| State | Condition | Visual treatment |
|---|---|---|
| **Error** | output starts with `"ERROR"` or `"VALIDATION FAILED"` | Red left-border accent, `XCircle` icon, red text, distinct red-tinted background |
| **No result** | output is null/undefined | Amber "No output captured" warning |
| **Success** | Everything else (green for `"SUCCESS"` prefix, default otherwise) | Current display |

### 3. Fix false-positive matching

Replace `.includes("SUCCESS")` / `.includes("ERROR")` with `.startsWith(...)` to avoid matching those strings inside JSON data.

### 4. Apply to both blocks

Same treatment for main exchange steps and child assistant trace steps.
