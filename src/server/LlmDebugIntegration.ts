import { addLlmLog, addLlmStep, updateLlmLog } from "@/server/models/debug";

export class LlmDebugIntegration {
  private logId: string;
  private startTime: number;

  constructor(request: any, parentId?: string, label?: string) {
    this.logId = addLlmLog(request, parentId, label);
    this.startTime = Date.now();
  }

  getLogId(): string {
    return this.logId;
  }

  onStepFinish(event: {
    stepNumber: number;
    finishReason: string;
    usage?: { inputTokens?: number; outputTokens?: number; totalTokens?: number };
    toolCalls?: Array<{ toolCallId: string; toolName: string; input: unknown }>;
    toolResults?: Array<{ toolCallId: string; toolName: string; output: unknown }>;
    text?: string;
    reasoningText?: string;
  }): void {
    addLlmStep({
      log_id: this.logId,
      step_number: event.stepNumber,
      finish_reason: event.finishReason ?? null,
      usage: JSON.stringify({
        inputTokens: event.usage?.inputTokens ?? 0,
        outputTokens: event.usage?.outputTokens ?? 0,
        totalTokens: event.usage?.totalTokens ?? 0,
      }),
      tool_calls: JSON.stringify(
        (event.toolCalls ?? []).map((tc) => ({
          toolCallId: tc.toolCallId,
          toolName: tc.toolName,
          input: tc.input,
        }))
      ),
      tool_results: JSON.stringify(
        (event.toolResults ?? []).map((tr) => ({
          toolCallId: tr.toolCallId,
          toolName: tr.toolName,
          output: tr.output,
        }))
      ),
      text: event.text ?? null,
      duration_ms: Date.now() - this.startTime,
    });
  }

  onFinish(event: {
    finishReason: string;
    usage?: { inputTokens?: number; outputTokens?: number; totalTokens?: number };
    totalUsage?: { inputTokens?: number; outputTokens?: number; totalTokens?: number };
    steps?: unknown[];
    text?: string;
  }): void {
    updateLlmLog(this.logId, event, Date.now() - this.startTime, 'SUCCESS');
  }

  onError(error: Error): void {
    updateLlmLog(
      this.logId,
      { error: error.message, stack: error.stack },
      Date.now() - this.startTime,
      'ERROR'
    );
  }
}
