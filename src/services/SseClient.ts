import type { DialogueOption } from "@/types/dialogue";

export interface SseCallbacks {
  onToken?: (token: string) => void;
  onMessageStart?: (data: { speaker: string; type: string }) => void;
  onMessageEnd?: () => void;
  onWorldUpdate?: (data: { entityId: string; changes: Record<string, unknown> }) => void;
  onPlotUpdate?: (data: { plotId: string; status: string }) => void;
  onPlotCreate?: (data: { plotId: string; title: string }) => void;
  onOptions?: (options: DialogueOption[]) => void;
  onStreamingMessages?: (messages: { speaker: string; type: string; text: string }[]) => void;
  onParsed?: (data: { messages: { speaker: string; type: string; text: string }[]; options: DialogueOption[] | null }) => void;
  onStepStart?: (data: { stepId: string }) => void;
  onStepEnd?: (data: { stepId: string }) => void;
  onError?: (message: string) => void;
  onDone?: () => void;
}

export class SseClient {
  private abortController: AbortController | null = null;

  async stream(
    url: string,
    body: Record<string, unknown>,
    callbacks: SseCallbacks
  ): Promise<void> {
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
      if (error instanceof DOMException && error.name === "AbortError") {
        return;
      }
      const message = error instanceof Error ? error.message : String(error);
      callbacks.onError?.(message);
    }
  }

  private dispatch(event: string, data: unknown, cb: SseCallbacks) {
    switch (event) {
      case "token":
        cb.onToken?.((data as { token: string }).token);
        break;
      case "message_start":
        cb.onMessageStart?.(data as { speaker: string; type: string });
        break;
      case "message_end":
        cb.onMessageEnd?.();
        break;
      case "world_update":
        cb.onWorldUpdate?.(data as { entityId: string; changes: Record<string, unknown> });
        break;
      case "plot_update":
        cb.onPlotUpdate?.(data as { plotId: string; status: string });
        break;
      case "plot_create":
        cb.onPlotCreate?.(data as { plotId: string; title: string });
        break;
      case "options":
        cb.onOptions?.((data as { options: DialogueOption[] }).options);
        break;
      case "streaming_messages":
        cb.onStreamingMessages?.((data as { messages: { speaker: string; type: string; text: string }[] }).messages);
        break;
      case "parsed":
        cb.onParsed?.(data as { messages: { speaker: string; type: string; text: string }[]; options: DialogueOption[] | null });
        break;
      case "step_start":
        cb.onStepStart?.(data as { stepId: string });
        break;
      case "step_end":
        cb.onStepEnd?.(data as { stepId: string });
        break;
      case "error":
        cb.onError?.((data as { message: string }).message);
        break;
      case "done":
        cb.onDone?.();
        break;
    }
  }

  abort() {
    this.abortController?.abort();
  }
}
