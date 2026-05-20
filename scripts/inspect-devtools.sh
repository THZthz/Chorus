#!/usr/bin/env bash

# Chorus — cinematic dialogue engine
# Copyright (C) 2026 Amias
#
# This program is free software: you can redistribute it and/or modify
# it under the terms of the GNU Affero General Public License as published by
# the Free Software Foundation, either version 3 of the License, or
# (at your option) any later version.
#
# This program is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
# GNU Affero General Public License for more details.
#
# You should have received a copy of the GNU Affero General Public License
# along with this program.  If not, see <https://www.gnu.org/licenses/>.

# Inspect devtools generations.json — readable LLM interaction
# Usage:
#   ./scripts/inspect-devtools.sh                                # Latest run, all steps (default)
#   ./scripts/inspect-devtools.sh --show-runs-summary            # Summary of all runs (time, step count, token usage)
#   ./scripts/inspect-devtools.sh --run N                        # Specific run by index (0-based, negative from end; N=-1, the latest run)
#   ./scripts/inspect-devtools.sh --run N --step M               # Specific step
#   ./scripts/inspect-devtools.sh --run N --step M --tool-result # Show tool call results (from the next step's input)
#
#   N: 0-based index (sorted by start time), negative counts from end (-1=latest)
#   N: can also be a run_id prefix (e.g. "20260513")
#   M: step_number (1-based, as stored in JSON)
#
# Use `--full` without `--tool-result` show full content of LLM's reasoning/text output, and tool input.
# Use `--full` with `--tool-result` show full content of tool-call result.


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
FULL_MODE=false       # --full: no truncation, wider output
TOOL_RESULT_MODE=false # --tool-result: show tool call results

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
    --full)
      FULL_MODE=true
      shift
      ;;
    --tool-result)
      TOOL_RESULT_MODE=true
      shift
      ;;
    *)
      # Backward compat: bare arg is run spec
      RUN_SPEC="$1"
      shift
      ;;
  esac
done

# Box fold width: content area width (excluding │ borders)
if $FULL_MODE; then
  BOX_WIDTH=$(tput cols 2>/dev/null || echo 120)
  ((BOX_WIDTH > 140)) && BOX_WIDTH=140
  BOX_WIDTH=$((BOX_WIDTH - 6))  # margin for indent + box borders
else
  BOX_WIDTH=72
fi

# Helper: repeat character N times (avoids 'tr' issues)
repeat() { printf "$1%.0s" $(seq 1 "$2"); }

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
  local RESULTS_MAP="${2:-{}}"  # JSON object: toolCallId → {toolName, output}
  local PREV_PROMPT_LEN="${3:-0}"  # previous step's prompt length (show only user msgs at indices >= this)

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

  # --- User messages (only new ones since previous step) ---
  local USER_MSGS=$(echo "$STEP" | jq -c --argjson prev "$PREV_PROMPT_LEN" '
    .input | fromjson | .prompt // [] | to_entries |
    map(select(.value.role == "user" and .key >= $prev)) |
    map({content: (.value.content |
      if type == "array" then map(.text // "") | join("\n")
      else .
      end
    )})
  ' 2>/dev/null)
  local USER_COUNT=$(echo "$USER_MSGS" | jq 'length' 2>/dev/null)
  if [[ "${USER_COUNT:-0}" -gt 0 ]]; then
    echo ""
    for ((ui=0; ui<USER_COUNT; ui++)); do
      local UCONTENT=$(echo "$USER_MSGS" | jq -r ".[$ui].content")
      if ! $FULL_MODE; then
        if [[ ${#UCONTENT} -gt 300 ]]; then
          UCONTENT="${UCONTENT:0:300}…"
        fi
      fi
      if [[ "$USER_COUNT" -gt 1 ]]; then
        echo "  · user[$((ui + 1))]: ${UCONTENT}"
      else
        echo "  · user: ${UCONTENT}"
      fi
    done
  fi

  # --- Reasoning ---
  local REASONING=$(echo "$STEP" | jq -r '
    .output | fromjson | .reasoningParts // [] | map(.text) | join("")
  ' 2>/dev/null)
  if [[ -n "$REASONING" ]]; then
    echo ""
    local rpad=$((BOX_WIDTH - 10))
    echo "  ┌─ Reasoning $(repeat '─' $rpad)┐"
    echo "$REASONING" | fold -s -w $BOX_WIDTH | while IFS= read -r line; do
      printf "  │ %-*s │\n" $BOX_WIDTH "$line"
    done
    echo "  └$(repeat '─' $((BOX_WIDTH + 2)))┘"
  fi

  # --- Text output ---
  local TEXT_OUT=$(echo "$STEP" | jq -r '
    .output | fromjson | .textParts // [] | map(.text) | join("")
  ' 2>/dev/null)
  if [[ -n "$TEXT_OUT" ]]; then
    echo ""
    local tpad=$((BOX_WIDTH - 12))
    echo "  ┌─ Text Output $(repeat '─' $tpad)┐"
    echo "$TEXT_OUT" | fold -s -w $BOX_WIDTH | while IFS= read -r line; do
      printf "  │ %-*s │\n" $BOX_WIDTH "$line"
    done
    echo "  └$(repeat '─' $((BOX_WIDTH + 2)))┘"
  fi

  # --- Tool calls ---
  local TOOL_COUNT=$(echo "$STEP" | jq -r '.output | fromjson | (.toolCalls // []) | length')
  if [[ "$TOOL_COUNT" -gt 0 ]]; then
    echo ""
    local tpad=$((BOX_WIDTH - 14 - ${#TOOL_COUNT}))
    echo "  ┌─ Tool Calls (${TOOL_COUNT}) $(repeat '─' $tpad)┐"
    for ((j=0; j<TOOL_COUNT; j++)); do
      local TOOL_NAME=$(echo "$STEP" | jq -r ".output | fromjson | .toolCalls[$j].toolName")
      local TOOL_ARGS=$(echo "$STEP" | jq -r ".output | fromjson | .toolCalls[$j].input | fromjson")

      printf "  │ %*s │\n" $BOX_WIDTH ""
      printf "  │  ▶ %s\n" "$TOOL_NAME"

      if [[ "$TOOL_NAME" == "generateDialogueStep" ]]; then
        local MSG_COUNT=$(echo "$TOOL_ARGS" | jq '.messages | length')
        local OPT_COUNT=$(echo "$TOOL_ARGS" | jq '.options | length')
        printf "  │    messages: %d, options: %d\n" "$MSG_COUNT" "$OPT_COUNT"
        if $FULL_MODE; then
          echo "$TOOL_ARGS" | jq -r '
            .messages[]? |
            "  │    [\(.type)] \(.speaker): \(.text)"
          '
          echo "$TOOL_ARGS" | jq -r '
            .options[]? |
            "  │    → \(.text)\(.hintBefore // "" | if . != "" then "  [\(.)]" else "" end)\(
              if .check then "  <\(.check.skill) DC\(.check.difficulty) \(.check.difficultyText)>" else "" end
            )"
          '
        else
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
        fi
      else
        if $FULL_MODE; then
          echo "$TOOL_ARGS" | jq -r '
            to_entries | map("\(.key)=\(.value | tostring)") | join(", ")
          ' | fold -s -w $BOX_WIDTH | while IFS= read -r line; do
            printf "  │    %s\n" "$line"
          done
        else
          echo "$TOOL_ARGS" | jq -r '
            to_entries | map("\(.key)=\(.value | tostring)") | join(", ")
          ' | fold -s -w 64 | while IFS= read -r line; do
            printf "  │    %s\n" "$line"
          done
        fi
      fi
    done
    echo "  └$(repeat '─' $((BOX_WIDTH + 2)))┘"

    # --- Tool results (if --tool-result) ---
    if $TOOL_RESULT_MODE; then
      # Write results map to temp file to avoid pipe/echo limits with large JSON
      local R_TMP=$(mktemp)
      printf '%s' "$RESULTS_MAP" > "$R_TMP"
      local RESULT_COUNT=$(jq 'length' "$R_TMP" 2>/dev/null)
      RESULT_COUNT="${RESULT_COUNT:-0}"
      if [[ "$RESULT_COUNT" -gt 0 ]]; then
        echo ""
        local res_hdr="Tool Results (${RESULT_COUNT})"
        local respad=$((BOX_WIDTH - ${#res_hdr}))
        echo "  ┌─ ${res_hdr} $(repeat '─' $respad)┐"
        for ((j=0; j<TOOL_COUNT; j++)); do
          local TC_ID=$(jq -r ".output | fromjson | .toolCalls[$j].toolCallId" <<<"$STEP")
          local RESULT=$(jq -r --arg id "$TC_ID" '.[$id] // empty' "$R_TMP" 2>/dev/null)
          if [[ -n "$RESULT" && "$RESULT" != "null" ]]; then
            local RT_NAME=$(jq -r '.toolName // "unknown"' <<<"$RESULT")
            local RT_OUTPUT=$(jq -r '.output' <<<"$RESULT")
            printf "  │ %*s │\n" $BOX_WIDTH ""
            printf "  │  ◀ %s  (result)\n" "$RT_NAME"
            if $FULL_MODE; then
              jq -r 'to_entries | map("\(.key)=\(.value | tostring)") | join(", ")' <<<"$RT_OUTPUT" | fold -s -w $BOX_WIDTH | while IFS= read -r line; do
                printf "  │    %s\n" "$line"
              done
            else
              local RT_OUT_FMT=$(jq -r 'to_entries | map("\(.key)=\(.value | tostring)") | join(", ")' <<<"$RT_OUTPUT")
              if [[ ${#RT_OUT_FMT} -gt 200 ]]; then
                RT_OUT_FMT="${RT_OUT_FMT:0:200}…"
              fi
              printf '%s' "$RT_OUT_FMT" | fold -s -w $BOX_WIDTH | while IFS= read -r line; do
                printf "  │    %s\n" "$line"
              done
            fi
          fi
        done
        echo "  └$(repeat '─' $((BOX_WIDTH + 2)))┘"
      fi
      rm -f "$R_TMP"
    fi
  fi
}

# --- Render steps ---
if [[ -n "$STEP_SPEC" ]]; then
  STEP_JSON=$(jq -r --argjson sn "$STEP_SPEC" '.[] | select(.step_number == $sn)' <<<"$STEPS")
  if [[ -z "$STEP_JSON" || "$STEP_JSON" == "null" ]]; then
    echo "ERROR: Step $STEP_SPEC not found in run" >&2
    exit 1
  fi
  STEP_NUM=$(jq -r '.step_number' <<<"$STEP_JSON")
  NEXT_IDX=$((STEP_NUM))  # step_number is 1-based, array index is 0-based
  RESULTS=$(jq -n -c --slurpfile d "$FILE" --arg rid "$RUN_ID" --argjson sn "$STEP_NUM" '
    $d[0].steps | map(select(.run_id == $rid)) | sort_by(.step_number) |
    .[$sn] | .input | fromjson | .prompt // [] |
    map(select(.role == "tool")) | map(.content[]) |
    map({key: .toolCallId, value: {toolName: .toolName, output: .output}}) |
    from_entries
  ' 2>/dev/null)
  RESULTS="${RESULTS:-{\}}"
  # Compute previous step's prompt length for delta display
  PREV_LEN=0
  if [[ "$STEP_NUM" -gt 1 ]]; then
    PREV_LEN=$(jq -r --argjson sn "$((STEP_NUM - 1))" '.[] | select(.step_number == $sn) | (.input | fromjson | .prompt | length)' <<<"$STEPS" 2>/dev/null)
    PREV_LEN="${PREV_LEN:-0}"
  fi
  render_step "$STEP_JSON" "$RESULTS" "$PREV_LEN"
else
  # Precompute results per step using one jq pass — avoids large-JSON-in-bash issues
  # Produces a JSON array: [[results_for_step1], [results_for_step2], ...]
  RESULTS_FILE=$(mktemp)
  jq -n -c --slurpfile d "$FILE" --arg rid "$RUN_ID" '
    [$d[0].steps | map(select(.run_id == $rid)) | sort_by(.step_number) |
     to_entries[] |
     .value.input | fromjson | .prompt // [] |
     map(select(.role == "tool")) | map(.content[]) |
     map({key: .toolCallId, value: {toolName: .toolName, output: .output}}) |
     from_entries]
  ' > "$RESULTS_FILE" 2>/dev/null
  # Ensure RESULTS_FILE has valid content (empty array if jq failed)
  if [[ ! -s "$RESULTS_FILE" ]]; then echo "[]" > "$RESULTS_FILE"; fi

  PREV_LEN=0
  for ((i=0; i<STEP_COUNT; i++)); do
    S=$(jq -r ".[$i]" <<<"$STEPS")
    # Results for step i are in step i+1 (because step i's tool calls resolve in step i+1's input)
    NEXT_I=$((i + 1))
    RESULTS=$(jq -c ".[$NEXT_I] // {}" "$RESULTS_FILE" 2>/dev/null)
    RESULTS="${RESULTS:-{\}}"
    render_step "$S" "$RESULTS" "$PREV_LEN"
    # Track this step's prompt length for the next iteration's delta
    PREV_LEN=$(echo "$S" | jq -r '(.input | fromjson | .prompt | length) // 0' 2>/dev/null)
    PREV_LEN="${PREV_LEN:-0}"
  done
  rm -f "$RESULTS_FILE"
fi

echo ""
echo "══════════════════════════════════════════════════════════════════"
echo "END OF RUN: $RUN_ID"
echo "══════════════════════════════════════════════════════════════════"
