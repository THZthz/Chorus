import type { DialogueOption } from "@/types/dialogue";
import type { Plot } from "@/types/plot";

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
  changes: Partial<Pick<Plot, "status" | "description" | "involvedLocations" | "involvedCharacters" | "childPlots">>;
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
  | DoneEvent;

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
}
