/**
 * Chorus — cinematic RPG-style dialogue engine
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

import type { Response } from "express";
import type { DialogueOption } from "@/types/dialogue";
import type { SseEventName, SseEventMap } from "@/shared/events";

export type EventEmitter = TurnEventEmitter;

// A message payload from the LLM before it gets a persistent ID.
export interface StreamingMessage {
  speaker: string;
  type: string;
  text: string;
  metadata?: Record<string, unknown>;
}

// Manages SSE output for a single turn.
// Tools call emit* methods during their execute phase.
// The stream writer calls start/finish to manage the SSE lifecycle.
export class TurnEventEmitter {
  private readonly res: Response;

  constructor(res: Response) {
    this.res = res;
  }

  private send<T extends SseEventName>(event: T, data: Omit<SseEventMap[T], "type">) {
    this.res.write(`event: ${event}\ndata: ${JSON.stringify({ ...data, type: event })}\n\n`);
  }

  // ── Lifecycle ──

  startStep(stepId: string) {
    this.send("step_start", { stepId });
  }

  finish() {
    this.send("done", {});
    this.res.end();
  }

  // ── Tool-triggered events (immediately visible to user) ──

  emitStreamingReset() {
    this.send("streaming_reset", {});
  }

  emitStreamingMessages(messages: StreamingMessage[]) {
    this.send("streaming_messages", { messages });
  }

  emitOptions(options: DialogueOption[]) {
    this.send("options", { options: options as unknown as Record<string, unknown>[] });
  }

  emitParsed(messages: StreamingMessage[], options: DialogueOption[]) {
    this.send("parsed", { messages, options: options as unknown as Record<string, unknown>[] });
  }

  emitError(message: string) {
    this.send("error", { message });
  }

  emitTimeUpdate(day: number, segment: number, segmentsAdvanced: number) {
    this.send("time_update", { day, segment, segmentsAdvanced });
  }

  emitRollResult(data: {
    skill: string;
    difficulty: number;
    dice: number[];
    total: number;
    statBonus: number;
    success: boolean;
    matchedConditions: Array<{
      expression: string;
      label?: string;
      color?: string;
      stepId?: string;
    }>;
  }) {
    this.send("roll_result", data);
  }
}
