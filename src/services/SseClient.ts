import type { DialogueOption } from "@/types/dialogue";
import type { StreamingMessage } from "@/shared/events";

export interface SseCallbacks {
  onStepStart?: (data: { stepId: string }) => void;
  onWorldUpdate?: (data: { entityId: string; changes: Record<string, unknown> }) => void;
  onPlotUpdate?: (data: { plotId: string; status: string }) => void;
  onPlotCreate?: (data: { plotId: string; title: string }) => void;
  onStreamingMessages?: (messages: StreamingMessage[]) => void;
  onOptions?: (options: DialogueOption[]) => void;
  onParsed?: (data: { messages: StreamingMessage[]; options: DialogueOption[] }) => void;
  onError?: (message: string) => void;
  onDone?: () => void;
}

export class SseClient {
  private abortController: AbortController | null = null;

  get signal(): AbortSignal | null {
    return this.abortController?.signal ?? null;
  }

  async stream(
    url: string,
    body: Record<string, unknown>,
    callbacks: SseCallbacks,
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

  private dispatch(event: string, data: any, cb: SseCallbacks) {
    switch (event) {
      case "step_start":
        console.log(`[sse] event=step_start stepId=${data.stepId}`);
        cb.onStepStart?.(data);
        break;
      case "world_update":
        console.log(`[sse] event=world_update entityId=${data.entityId}`);
        cb.onWorldUpdate?.(data);
        break;
      case "plot_update":
        console.log(`[sse] event=plot_update plotId=${data.plotId}`);
        cb.onPlotUpdate?.(data);
        break;
      case "plot_create":
        console.log(`[sse] event=plot_create plotId=${data.plotId}`);
        cb.onPlotCreate?.(data);
        break;
      case "streaming_messages":
        cb.onStreamingMessages?.(data.messages);
        break;
      case "options":
        console.log(`[sse] event=options count=${data.options?.length ?? 0}`);
        cb.onOptions?.(data.options);
        break;
      case "parsed":
        console.log(`[sse] event=parsed msgs=${data.messages?.length ?? 0} opts=${data.options?.length ?? 0}`);
        cb.onParsed?.(data);
        break;
      case "error":
        console.error(`[sse] event=error message=${data.message}`);
        cb.onError?.(data.message);
        break;
      case "done":
        console.log(`[sse] event=done`);
        cb.onDone?.();
        break;
    }
  }

  abort() {
    this.abortController?.abort();
  }
}
