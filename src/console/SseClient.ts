/**
 * Chorus — cinematic dialogue engine
 * Copyright (C) 2026 Amias
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
import type { SseEventMap } from "@/shared/events";

type CallbackData<T extends keyof SseEventMap> = Omit<SseEventMap[T], "type">;

export interface SseCallbacks {
  onStepStart?: (data: CallbackData<"step_start">) => void;
  onStreamingMessages?: (messages: CallbackData<"streaming_messages">["messages"]) => void;
  onStreamingReset?: () => void;
  onOptions?: (options: CallbackData<"options">["options"]) => void;
  onParsed?: (data: CallbackData<"parsed">) => void;
  onError?: (message: CallbackData<"error">["message"]) => void;
  onDone?: () => void;
  onRollResult?: (data: CallbackData<"roll_result">) => void;
}

export class ConsoleSseClient {
  private abortController: AbortController | null = null;

  abort() {
    this.abortController?.abort();
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
        const err = await response.text().catch(() => response.statusText);
        callbacks.onError?.(err || response.statusText);
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
      if (error instanceof Error && error.name === "AbortError") {
        return;
      }
      const message = error instanceof Error ? error.message : String(error);
      callbacks.onError?.(message);
    }
  }

  private dispatch(event: string, data: any, cb: SseCallbacks) {
    switch (event) {
      case "step_start":
        cb.onStepStart?.(data);
        break;
      case "streaming_messages":
        cb.onStreamingMessages?.(data.messages);
        break;
      case "streaming_reset":
        cb.onStreamingReset?.();
        break;
      case "options":
        cb.onOptions?.(data.options);
        break;
      case "parsed":
        cb.onParsed?.(data);
        break;
      case "error":
        cb.onError?.(data.message);
        break;
      case "done":
        cb.onDone?.();
        break;
      case "roll_result":
        cb.onRollResult?.(data);
        break;
      // World/plot/time/time_update events ignored for console client
    }
  }
}
