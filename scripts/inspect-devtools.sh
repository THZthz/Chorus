#!/usr/bin/env bash
# Inspect devtools generations.json — readable LLM interaction
# Usage:
#   ./scripts/inspect-devtools.sh                                # latest run, all steps
#   ./scripts/inspect-devtools.sh --show-runs-summary            # summary of all runs
#   ./scripts/inspect-devtools.sh --run N                        # specific run, all steps
#   ./scripts/inspect-devtools.sh --run N --step M               # specific step
#   N: 0-based index (sorted by start time), negative counts from end (-1=latest)
#   N: can also be a run_id prefix (e.g. "20260513")
#   M: step_number (1-based, as stored in JSON)
set -euo pipefail

FILE=".devtools/generations.json"

if [[ ! -f "$FILE" ]]; then
  echo "ERROR: $FILE not found" >&2
  exit 1
fi

# --- Argument parsing ---
MODE="inspect"       # inspect | summary
RUN_SPEC=""           # run index, negative index, or run_id prefix
STEP_SPEC=""          # step number (1-based)

while [[ $# -gt 0 ]]; do
  case "$1" in
    --show-runs-summary)
      MODE="summary"
      shift
      ;;
    --run)
      RUN_SPEC="$2"
      shift 2
      ;;
    --step)
      STEP_SPEC="$2"
      shift 2
      ;;
    *)
      # Backward compat: bare arg is run spec
      RUN_SPEC="$1"
      shift
      ;;
  esac
done

# --- Resolve run_id ---
resolve_run() {
  local spec="$1"
  local rid
  if [[ "$spec" =~ ^-?[0-9]+$ ]]; then
    rid=$(jq -r --argjson idx "$spec" '
      .runs | sort_by(.started_at) | if $idx < 0 then .[$idx] else .[$idx] end | .id // empty
    ' "$FILE")
  elif [[ -n "$spec" ]]; then
    # Treat as run_id prefix — find first match sorted by time
    rid=$(jq -r --arg prefix "$spec" '
      [.runs | sort_by(.started_at)[] | select(.id | startswith($prefix))] | first | .id // empty
    ' "$FILE")
  fi
  echo "${rid:-}"
}

# --- Runs summary mode ---
if [[ "$MODE" == "summary" ]]; then
  echo "═══ RUNS SUMMARY ═══"
  echo ""
  jq -n -r --slurpfile d "$FILE" '
    $d[0].runs | sort_by(.started_at) | to_entries[] |
    .key as $idx |
    .value.id as $rid |
    .value.started_at as $ts |
    [
      $idx,
      $rid,
      $ts[0:16],
      ( [$d[0].steps[] | select(.run_id == $rid)] | length | tostring ),
      ( [$d[0].steps[] | select(.run_id == $rid) | (.output | fromjson | .usage.inputTokens.total // 0)] | add | tostring ),
      ( [$d[0].steps[] | select(.run_id == $rid) | (.output | fromjson | .usage.outputTokens.total // 0)] | add | tostring ),
      ( [$d[0].steps[] | select(.run_id == $rid) | (.output | fromjson | .usage.inputTokens.cacheRead // 0)] | add | tostring )
    ] | join("\t")
  ' | {
    printf "%-6s %-32s %-16s %-6s %-10s %-10s %-10s\n" "INDEX" "RUN_ID" "STARTED" "STEPS" "IN_TOK" "OUT_TOK" "CACHE_TOK"
    printf "%-6s %-32s %-16s %-6s %-10s %-10s %-10s\n" "------" "--------------------------------" "----------------" "------" "----------" "----------" "----------"
    while IFS=$'\t' read -r idx rid ts steps in_tok out_tok cache_tok; do
      printf "%-6s %-32s %-16s %-6s %-10s %-10s %-10s\n" "$idx" "$rid" "$ts" "$steps" "$in_tok" "$out_tok" "$cache_tok"
    done
  }
  exit 0
fi

# --- Inspect mode: resolve run ---
RUN_ID=$(resolve_run "${RUN_SPEC:--1}")
if [[ -z "$RUN_ID" ]]; then
  echo "ERROR: No run found for '${RUN_SPEC:--1}'" >&2
  echo "Available runs:" >&2
  jq -r '.runs | to_entries[] | "  [\(.key)] \(.id) — \(.started_at)"' "$FILE" >&2
  exit 1
fi

RUN_STARTED=$(jq -r --arg rid "$RUN_ID" '.runs[] | select(.id == $rid) | .started_at' "$FILE")
STEPS=$(jq -c --arg rid "$RUN_ID" '[.steps[] | select(.run_id == $rid)] | sort_by(.step_number)' "$FILE")
STEP_COUNT=$(echo "$STEPS" | jq 'length')

echo "══════════════════════════════════════════════════════════════════"
echo "RUN: $RUN_ID"
echo "STARTED: $RUN_STARTED"
echo "STEPS: $STEP_COUNT"
echo "══════════════════════════════════════════════════════════════════"

# --- Step rendering function ---
render_step() {
  local STEP="$1"

  local STEP_NUM=$(echo "$STEP" | jq -r '.step_number')
  local MODEL=$(echo "$STEP" | jq -r '.model_id // "unknown"')
  local DURATION=$(echo "$STEP" | jq -r '.duration_ms // 0')
  local ERROR=$(echo "$STEP" | jq -r '.error // "none"')
  local FINISH=$(echo "$STEP" | jq -r '.output | fromjson | .finishReason.unified // "unknown"')
  local USAGE=$(echo "$STEP" | jq -r '.output | fromjson | .usage // {}')
  local INPUT_TOKENS=$(echo "$USAGE" | jq -r '.inputTokens.total // 0')
  local OUTPUT_TOKENS=$(echo "$USAGE" | jq -r '.outputTokens.total // 0')
  local CACHE_READ=$(echo "$USAGE" | jq -r '.inputTokens.cacheRead // 0')

  echo ""
  echo "── Step ${STEP_NUM} ── model=${MODEL}  duration=${DURATION}ms  finish=${FINISH}  tokens(in=${INPUT_TOKENS} out=${OUTPUT_TOKENS} cache=${CACHE_READ})"

  if [[ "$ERROR" != "null" && "$ERROR" != "none" ]]; then
    echo "  ⚠ ERROR: $(echo "$STEP" | jq -c '.error')"
  fi

  # --- User input ---
  local INPUT_MSG=$(echo "$STEP" | jq -r '
    .input | fromjson | .prompt // [] | map(select(.role == "user")) | last
  ' 2>/dev/null)
  if [[ -n "$INPUT_MSG" && "$INPUT_MSG" != "null" ]]; then
    local CONTENT=$(echo "$INPUT_MSG" | jq -r '
      .content |
      if type == "array" then map(.text // "") | join(" | ")
      else tostring
      end
    ')
    local CONTENT_TRUNC=$(echo "$CONTENT" | head -c 300)
    if [[ ${#CONTENT} -gt 300 ]]; then
      CONTENT_TRUNC="${CONTENT_TRUNC}…"
    fi
    echo ""
    echo "  · user: ${CONTENT_TRUNC}"
  fi

  # --- Reasoning ---
  local REASONING=$(echo "$STEP" | jq -r '
    .output | fromjson | .reasoningParts // [] | map(.text) | join("")
  ' 2>/dev/null)
  if [[ -n "$REASONING" ]]; then
    echo ""
    echo "  ┌─ Reasoning ─────────────────────────────┐"
    echo "$REASONING" | fold -s -w 74 | while IFS= read -r line; do
      printf "  │ %-72s │\n" "$line"
    done
    echo "  └──────────────────────────────────────────┘"
  fi

  # --- Text output ---
  local TEXT_OUT=$(echo "$STEP" | jq -r '
    .output | fromjson | .textParts // [] | map(.text) | join("")
  ' 2>/dev/null)
  if [[ -n "$TEXT_OUT" ]]; then
    echo ""
    echo "  ┌─ Text Output ────────────────────────────┐"
    echo "$TEXT_OUT" | fold -s -w 74 | while IFS= read -r line; do
      printf "  │ %-72s │\n" "$line"
    done
    echo "  └──────────────────────────────────────────┘"
  fi

  # --- Tool calls ---
  local TOOL_COUNT=$(echo "$STEP" | jq -r '.output | fromjson | (.toolCalls // []) | length')
  if [[ "$TOOL_COUNT" -gt 0 ]]; then
    echo ""
    echo "  ┌─ Tool Calls (${TOOL_COUNT}) ─────────────────────┐"
    for ((j=0; j<TOOL_COUNT; j++)); do
      local TOOL_NAME=$(echo "$STEP" | jq -r ".output | fromjson | .toolCalls[$j].toolName")
      local TOOL_ARGS=$(echo "$STEP" | jq -r ".output | fromjson | .toolCalls[$j].input | fromjson")

      echo "  │                                                │"
      printf "  │  ▶ %s\n" "$TOOL_NAME"

      if [[ "$TOOL_NAME" == "generateDialogueStep" ]]; then
        local MSG_COUNT=$(echo "$TOOL_ARGS" | jq '.messages | length')
        local OPT_COUNT=$(echo "$TOOL_ARGS" | jq '.options | length')
        printf "  │    messages: %d, options: %d\n" "$MSG_COUNT" "$OPT_COUNT"
        echo "$TOOL_ARGS" | jq -r '
          .messages[]? |
          "  │    [\(.type)] \(.speaker): \(.text[:120])\(if (.text | length) > 120 then "…" else "" end)"
        '
        echo "$TOOL_ARGS" | jq -r '
          .options[]? |
          "  │    → \(.text[:100])\(if (.text | length) > 100 then "…" else "" end)\(.hintBefore // "" | if . != "" then "  [\(.)]" else "" end)\(
            if .check then "  <\(.check.skill) DC\(.check.difficulty) \(.check.difficultyText)>" else "" end
          )"
        '
      else
        echo "$TOOL_ARGS" | jq -r '
          to_entries | map("\(.key)=\(.value | tostring)") | join(", ")
        ' | fold -s -w 64 | while IFS= read -r line; do
          printf "  │    %s\n" "$line"
        done
      fi
    done
    echo "  └──────────────────────────────────────────────┘"
  fi
}

# --- Render steps ---
if [[ -n "$STEP_SPEC" ]]; then
  # Single step mode
  STEP_JSON=$(echo "$STEPS" | jq -r --argjson sn "$STEP_SPEC" '.[] | select(.step_number == $sn)')
  if [[ -z "$STEP_JSON" || "$STEP_JSON" == "null" ]]; then
    echo "ERROR: Step $STEP_SPEC not found in run" >&2
    exit 1
  fi
  render_step "$STEP_JSON"
else
  # All steps
  for ((i=0; i<STEP_COUNT; i++)); do
    S=$(echo "$STEPS" | jq -r ".[$i]")
    render_step "$S"
  done
fi

echo ""
echo "══════════════════════════════════════════════════════════════════"
echo "END OF RUN: $RUN_ID"
echo "══════════════════════════════════════════════════════════════════"
