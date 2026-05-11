#!/usr/bin/env bash
# Inspect devtools generations.json for GM tool-calling analysis
set -euo pipefail

FILE=".devtools/generations.json"

echo "=== RUNS ==="
jq -r '.runs[] | "Run \(.id) — started \(.started_at)"' "$FILE"

echo ""
echo "=== STEPS OVERVIEW ==="
jq -r '.steps[] | "Step \(.step_number) | type=\(.type) | model=\(.model_id) | duration=\(.duration_ms)ms | error=\(.error // "none")"' "$FILE"

echo ""
echo "=== TOOL CALLS PER STEP ==="
jq -r '
  .steps[] |
  "--- Step \(.step_number) ---" as $header |
  [
    ($header),
    ( .output | fromjson | .toolCalls[]? |
      "  \(.toolName): \(.input | fromjson | to_entries | map("\(.key)") | join(", "))"
    ),
    ( .output | fromjson | .textParts[]? |
      "  [text] \(.text)[0:200]"
    ),
    (if (.output | fromjson | .toolCalls | length) == 0 then "  (no tool calls, text only)" else empty end)
  ] | join("\n")
' "$FILE"

echo ""
echo "=== INPUT MESSAGES PER STEP (role + first 150 chars) ==="
jq -r '
  .steps[] |
  "--- Step \(.step_number) messages ---",
  (
    .input | fromjson | .prompt[]? |
    "  [\(.role)] \(.content | tostring)[0:150]"
  )
' "$FILE"

echo ""
echo "=== FINISH REASONS ==="
jq -r '.steps[] | "Step \(.step_number): finishReason=\(.output | fromjson | .finishReason)"' "$FILE"
