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

export interface StepStartEvent {
  stepId: string;
}

export interface StreamingMessagesEvent {
  messages: Array<{
    speaker: string;
    type: string;
    text: string;
    metadata?: Record<string, unknown>;
  }>;
}

export interface StreamingResetEvent {}

export interface OptionsEvent {
  options: Array<Record<string, unknown>>;
}

export interface ParsedEvent {
  messages: Array<{
    speaker: string;
    type: string;
    text: string;
    metadata?: Record<string, unknown>;
  }>;
  options: Array<Record<string, unknown>>;
}

export interface ErrorEvent {
  message: string;
}

export interface DoneEvent {}

export interface TimeUpdateEvent {
  day: number;
  segment: number;
  segmentsAdvanced: number;
}

export interface RollResultEvent {
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
}

export interface SseEventMap {
  step_start: StepStartEvent;
  streaming_messages: StreamingMessagesEvent;
  streaming_reset: StreamingResetEvent;
  options: OptionsEvent;
  parsed: ParsedEvent;
  error: ErrorEvent;
  done: DoneEvent;
  time_update: TimeUpdateEvent;
  roll_result: RollResultEvent;
}

export type SseEventName = keyof SseEventMap;
