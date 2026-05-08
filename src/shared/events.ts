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

import type { DialogueOption } from "@/types/dialogue";
import type { PlotPatch } from "@/types/plot";
import type { SceneState, Fact } from "@/types/entities";

// ── SSE Event Payloads ──

export interface StepStartEvent {
  type: "step_start";
  stepId: string;
}

export interface WorldUpdateEvent {
  type: "world_update";
  entityId: string;
  changes: Record<string, unknown>;
}

export interface PlotUpdateEvent {
  type: "plot_update";
  plotId: string;
  status: string;
}

export interface PlotCreateEvent {
  type: "plot_create";
  plotId: string;
  title: string;
  parentPlotId: string | null;
}

export interface PlotEditEvent {
  type: "plot_edit";
  plotId: string;
  changes: PlotPatch;
}

export interface StreamingMessagesEvent {
  type: "streaming_messages";
  messages: StreamingMessage[];
}

export interface OptionsEvent {
  type: "options";
  options: DialogueOption[];
}

export interface ParsedEvent {
  type: "parsed";
  messages: StreamingMessage[];
  options: DialogueOption[];
}

export interface ErrorEvent {
  type: "error";
  message: string;
}

export interface StreamingResetEvent {
  type: "streaming_reset";
}

export interface DoneEvent {
  type: "done";
}

export interface TimeUpdateEvent {
  type: "time_update";
  day: number;
  segment: number;
  segmentsAdvanced: number;
}

export interface SceneUpdateEvent {
  type: "scene_update";
  scene: SceneState;
}

export interface FactAddEvent {
  type: "fact_add";
  fact: Fact;
}

export interface FactUpdateEvent {
  type: "fact_update";
  factId: string;
  changes: Record<string, unknown>;
}

export interface FactRemoveEvent {
  type: "fact_remove";
  factId: string;
}

/** A message payload from the LLM before it gets a persistent ID. */
export interface StreamingMessage {
  speaker: string;
  type: string;
  text: string;
  metadata?: Record<string, unknown>;
}

export type SseEventPayload =
  | StepStartEvent
  | WorldUpdateEvent
  | PlotUpdateEvent
  | PlotCreateEvent
  | PlotEditEvent
  | StreamingMessagesEvent
  | StreamingResetEvent
  | OptionsEvent
  | ParsedEvent
  | ErrorEvent
  | DoneEvent
  | TimeUpdateEvent
  | SceneUpdateEvent
  | FactAddEvent
  | FactUpdateEvent
  | FactRemoveEvent;

export type SseEventType = SseEventPayload["type"];

/** Map from event type string to its payload type. */
export interface SseEventMap {
  step_start: StepStartEvent;
  world_update: WorldUpdateEvent;
  plot_update: PlotUpdateEvent;
  plot_create: PlotCreateEvent;
  plot_edit: PlotEditEvent;
  streaming_messages: StreamingMessagesEvent;
  streaming_reset: StreamingResetEvent;
  options: OptionsEvent;
  parsed: ParsedEvent;
  error: ErrorEvent;
  done: DoneEvent;
  time_update: TimeUpdateEvent;
  scene_update: SceneUpdateEvent;
  fact_add: FactAddEvent;
  fact_update: FactUpdateEvent;
  fact_remove: FactRemoveEvent;
}
