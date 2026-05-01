import type { Response } from "express";
import type { DialogueOption } from "@/types/dialogue";
import type { StreamingMessage } from "@/shared/events";

/**
 * Manages SSE output for a single turn.
 * Tools call emit* methods during their execute phase.
 * The stream writer calls start/finish to manage the SSE lifecycle.
 */
export class TurnEventEmitter {
  private res: Response | null;

  constructor(
    res: Response | null,
    public readonly stepId: string,
  ) {
    this.res = res;
  }

  private send(event: string, data: unknown) {
    if (!this.res) return;
    this.res.write(`event: ${event}\ndata: ${JSON.stringify(data)}\n\n`);
  }

  // ── Lifecycle ──

  startStep() {
    this.send("step_start", { stepId: this.stepId });
  }

  finish() {
    this.send("done", {});
    if (this.res) this.res.end();
  }

  // ── Tool-triggered events (immediately visible to user) ──

  emitWorldUpdate(entityId: string, changes: Record<string, unknown>) {
    this.send("world_update", { entityId, changes });
  }

  emitPlotUpdate(plotId: string, status: string) {
    this.send("plot_update", { plotId, status });
  }

  emitPlotCreate(plotId: string, title: string) {
    this.send("plot_create", { plotId, title });
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
}
