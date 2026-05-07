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

      const reader = response.body?.getReader();
      if (!reader) {
        callbacks.onError?.("No response body");
        return;
      }

      const decoder = new TextDecoder();
      let buffer = "";

      while (true) {
        const { done, value } = await reader.read();
        if (done) break;

        buffer += decoder.decode(value, { stream: true });
        const lines = buffer.split("\n");
        buffer = lines.pop() ?? "";

        let currentEvent = "";
        let currentData = "";

        for (const line of lines) {
          if (line.startsWith("event: ")) {
            currentEvent = line.slice(7).trim();
          } else if (line.startsWith("data: ")) {
            currentData = line.slice(6);
          } else if (line.trim() === "" && currentData) {
            try {
              const data = JSON.parse(currentData);
              this.dispatch(currentEvent, data, callbacks);
            } catch {
              // Skip unparseable data
            }
            currentEvent = "";
            currentData = "";
          }
        }
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
      // World/plot/time/scene events ignored for console client
    }
  }
}
