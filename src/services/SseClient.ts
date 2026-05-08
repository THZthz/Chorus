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

import { parseSseStream } from "@/shared/sse";
import type { DialogueOption } from "@/types/dialogue";
import type { StreamingMessage, SseEventMap } from "@/shared/events";

type CallbackData<T extends keyof SseEventMap> = Omit<SseEventMap[T], "type">;

export interface SseCallbacks {
  onStepStart?: (data: CallbackData<"step_start">) => void;
  onWorldUpdate?: (data: CallbackData<"world_update">) => void;
  onPlotUpdate?: (data: CallbackData<"plot_update">) => void;
  onPlotCreate?: (data: CallbackData<"plot_create">) => void;
  onPlotEdit?: (data: CallbackData<"plot_edit">) => void;
  onStreamingMessages?: (messages: CallbackData<"streaming_messages">["messages"]) => void;
  onStreamingReset?: () => void;
  onOptions?: (options: CallbackData<"options">["options"]) => void;
  onParsed?: (data: CallbackData<"parsed">) => void;
  onError?: (message: CallbackData<"error">["message"]) => void;
  onDone?: () => void;
  onTimeUpdate?: (data: CallbackData<"time_update">) => void;
  onSceneUpdate?: (data: CallbackData<"scene_update">) => void;
  onFactAdd?: (data: CallbackData<"fact_add">) => void;
  onFactUpdate?: (data: CallbackData<"fact_update">) => void;
  onFactRemove?: (data: CallbackData<"fact_remove">) => void;
}

export class SseClient {
  private abortController: AbortController | null = null;

  get signal(): AbortSignal | null {
    return this.abortController?.signal ?? null;
  }

  async stream(url: string, body: Record<string, unknown>, callbacks: SseCallbacks): Promise<void> {
    this.abortController = new AbortController();

    try {
      const response = await fetch(url, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
        signal: this.abortController.signal,
      });

      if (!response.ok) {
        const err = await response.json().catch(() => ({ error: response.statusText }));
        callbacks.onError?.(err.error || response.statusText);
        return;
      }

      if (!response.body) {
        callbacks.onError?.("No response body");
        return;
      }

      for await (const { event, data } of parseSseStream(response.body)) {
        this.dispatch(event, data, callbacks);
      }
    } catch (error: unknown) {
      if (error instanceof DOMException && error.name === "AbortError") {
        return;
      }
      const message = error instanceof Error ? error.message : String(error);
      callbacks.onError?.(message);
    }
  }

  private dispatch(event: string, data: any, cb: SseCallbacks) {
    switch (event) {
      case "step_start":
        console.trace(`[sse] event=step_start stepId=${data.stepId}`);
        cb.onStepStart?.(data);
        break;
      case "world_update":
        console.trace(`[sse] event=world_update entityId=${data.entityId}`);
        cb.onWorldUpdate?.(data);
        break;
      case "plot_update":
        console.trace(`[sse] event=plot_update plotId=${data.plotId}`);
        cb.onPlotUpdate?.(data);
        break;
      case "plot_create":
        console.trace(`[sse] event=plot_create plotId=${data.plotId}`);
        cb.onPlotCreate?.(data);
        break;
      case "plot_edit":
        console.trace(`[sse] event=plot_edit plotId=${data.plotId}`);
        cb.onPlotEdit?.(data);
        break;
      case "streaming_messages":
        cb.onStreamingMessages?.(data.messages);
        break;
      case "streaming_reset":
        console.trace(`[sse] event=streaming_reset`);
        cb.onStreamingReset?.();
        break;
      case "options":
        console.trace(`[sse] event=options count=${data.options?.length ?? 0}`);
        cb.onOptions?.(data.options);
        break;
      case "parsed":
        console.trace(
          `[sse] event=parsed msgs=${data.messages?.length ?? 0} opts=${data.options?.length ?? 0}`,
        );
        cb.onParsed?.(data);
        break;
      case "error":
        console.error(`[sse] event=error message=${data.message}`);
        cb.onError?.(data.message);
        break;
      case "done":
        console.trace(`[sse] event=done`);
        cb.onDone?.();
        break;
      case "time_update":
        console.trace(`[sse] event=time_update day=${data.day} segment=${data.segment}`);
        cb.onTimeUpdate?.(data);
        break;
      case "scene_update":
        console.trace(`[sse] event=scene_update`);
        cb.onSceneUpdate?.(data);
        break;
      case "fact_add":
        console.trace(`[sse] event=fact_add factId=${data.fact.id}`);
        cb.onFactAdd?.(data);
        break;
      case "fact_update":
        console.trace(`[sse] event=fact_update factId=${data.factId}`);
        cb.onFactUpdate?.(data);
        break;
      case "fact_remove":
        console.trace(`[sse] event=fact_remove factId=${data.factId}`);
        cb.onFactRemove?.(data);
        break;
    }
  }

  abort() {
    this.abortController?.abort();
  }
}
