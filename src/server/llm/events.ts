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

import type { Response } from "express";
import type { DialogueOption } from "@/types/dialogue";
import type { PlotPatch } from "@/types/plot";
import type { SseEventType, SseEventMap, StreamingMessage } from "@/shared/events";
import type { SceneState, EntityType, Fact } from "@/types/entities";

export type EventEmitter = TurnEventEmitter | NoopEventEmitter;

/**
 * Manages SSE output for a single turn.
 * Tools call emit* methods during their execute phase.
 * The stream writer calls start/finish to manage the SSE lifecycle.
 */
export class TurnEventEmitter {
  private readonly res: Response;

  constructor(
    res: Response,
    public readonly stepId: string,
  ) {
    this.res = res;
  }

  private send<T extends SseEventType>(event: T, data: Omit<SseEventMap[T], "type">) {
    this.res.write(`event: ${event}\ndata: ${JSON.stringify({ ...data, type: event })}\n\n`);
  }

  // ── Lifecycle ──

  startStep() {
    this.send("step_start", { stepId: this.stepId });
  }

  finish() {
    this.send("done", {});
    this.res.end();
  }

  // ── Tool-triggered events (immediately visible to user) ──

  emitWorldUpdate(entityId: string, changes: Record<string, unknown>) {
    this.send("world_update", { entityId, changes });
  }

  emitPlotCreate(plotId: string, title: string, parentPlotId: string | null) {
    this.send("plot_create", { plotId, title, parentPlotId });
  }

  emitPlotEdit(plotId: string, changes: PlotPatch) {
    this.send("plot_edit", { plotId, changes });
  }

  emitStreamingReset() {
    this.send("streaming_reset", {});
  }

  emitStreamingMessages(messages: StreamingMessage[]) {
    this.send("streaming_messages", { messages });
  }

  emitOptions(options: DialogueOption[]) {
    this.send("options", { options });
  }

  emitParsed(messages: StreamingMessage[], options: DialogueOption[]) {
    this.send("parsed", { messages, options });
  }

  emitError(message: string) {
    this.send("error", { message });
  }

  emitTimeUpdate(day: number, segment: number, segmentsAdvanced: number) {
    this.send("time_update", { day, segment, segmentsAdvanced });
  }

  emitSceneUpdate(scene: SceneState) {
    this.send("scene_update", { scene });
  }

  emitEntityCreate(entityId: string, entityType: EntityType, displayName: string) {
    this.send("entity_create", { entityId, entityType, displayName });
  }

  emitFactAdd(fact: Fact) {
    this.send("fact_add", { fact });
  }

  emitFactUpdate(factId: string, changes: Record<string, unknown>) {
    this.send("fact_update", { factId, changes });
  }

  emitFactRemove(factId: string) {
    this.send("fact_remove", { factId });
  }
}

/**
 * No-op event emitter for non-streaming batch generation.
 * Has the same public API as TurnEventEmitter but does nothing.
 */
export class NoopEventEmitter {
  readonly stepId: string;
  constructor(stepId: string) {
    this.stepId = stepId;
  }
  startStep() {}
  finish() {}
  emitWorldUpdate(_entityId: string, _changes: Record<string, unknown>) {}
  emitPlotCreate(_plotId: string, _title: string, _parentPlotId: string | null) {}
  emitPlotEdit(_plotId: string, _changes: Record<string, unknown>) {}
  emitStreamingReset() {}
  emitStreamingMessages(_messages: unknown[]) {}
  emitOptions(_options: unknown[]) {}
  emitParsed(_messages: unknown[], _options: unknown[]) {}
  emitError(_message: string) {}
  emitTimeUpdate(_day: number, _segment: number, _segmentsAdvanced: number) {}
  emitSceneUpdate(_scene: unknown) {}
  emitEntityCreate(_entityId: string, _entityType: string, _displayName: string) {}
  emitFactAdd(_fact: unknown) {}
  emitFactUpdate(_factId: string, _changes: Record<string, unknown>) {}
  emitFactRemove(_factId: string) {}
}
