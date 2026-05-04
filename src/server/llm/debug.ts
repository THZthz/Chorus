/**
 * Elysian Dialogue — cinematic RPG-style dialogue engine
 * Copyright (C) 2026  Amias
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU Affero General Public License for more details.
 *
 * You should have received a copy of the GNU Affero General Public License
 * along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */

import { addLlmLog, addLlmStep, updateLlmLog } from "@/server/models/debug";

interface StepData {
  stepNumber: number;
  finishReason: string;
  usage?: { inputTokens?: number; outputTokens?: number; totalTokens?: number };
  toolCalls?: { toolCallId: string; toolName: string; input: unknown }[];
  toolResults?: { toolCallId: string; toolName: string; output: unknown }[];
  text?: string;
}

interface FinishData {
  finishReason: string;
  usage?: { inputTokens?: number; outputTokens?: number; totalTokens?: number };
  totalUsage?: { inputTokens?: number; outputTokens?: number; totalTokens?: number };
  steps?: unknown[];
  text?: string;
}

export class LlmDebugIntegration {
  private logId: string;
  private startTime: number;

  constructor(request: unknown, parentId?: string, label?: string) {
    this.logId = addLlmLog(request, parentId, label);
    this.startTime = Date.now();
  }

  onStepFinish(event: StepData) {
    addLlmStep({
      log_id: this.logId,
      step_number: event.stepNumber,
      finish_reason: event.finishReason ?? null,
      usage: JSON.stringify({
        inputTokens: event.usage?.inputTokens ?? 0,
        outputTokens: event.usage?.outputTokens ?? 0,
        totalTokens: event.usage?.totalTokens ?? 0,
      }),
      tool_calls: JSON.stringify(
        (event.toolCalls ?? []).map((tc) => ({
          toolCallId: tc.toolCallId,
          toolName: tc.toolName,
          input: tc.input,
        })),
      ),
      tool_results: JSON.stringify(
        (event.toolResults ?? []).map((tr) => ({
          toolCallId: tr.toolCallId,
          toolName: tr.toolName,
          output: tr.output,
        })),
      ),
      text: event.text ?? null,
      duration_ms: Date.now() - this.startTime,
    });
  }

  onFinish(event: FinishData) {
    updateLlmLog(this.logId, event, Date.now() - this.startTime, "SUCCESS");
  }

  onError(error: Error) {
    updateLlmLog(
      this.logId,
      { error: error.message, stack: error.stack },
      Date.now() - this.startTime,
      "ERROR",
    );
  }
}
