import type { Response } from "express";
import type { DialogueOption } from "@/types/dialogue";

/** Manages SSE output for a single turn. Tools call emit* methods; the stream writer calls feedToken/done. */
export class TurnEventEmitter {
  private res: Response;
  private accumulated = "";

  constructor(res: Response, public readonly stepId: string) {
    this.res = res;
  }

  private write(evt: string, data: unknown) {
    this.res.write(`event: ${evt}\ndata: ${JSON.stringify(data)}\n\n`);
  }

  // ── called by the streamText loop ──

  startStep() {
    this.write("step_start", { stepId: this.stepId });
  }

  feedToken(token: string) {
    this.accumulated += token;
    this.write("token", { token });
  }

  finish() {
    this.write("done", {});
    this.res.end();
  }

  getText(): string {
    return this.accumulated;
  }

  // ── called by tool execute functions ──

  emitWorldUpdate(entityId: string, changes: Record<string, unknown>) {
    this.write("world_update", { entityId, changes });
  }

  emitPlotUpdate(plotId: string, status: string) {
    this.write("plot_update", { plotId, status });
  }

  emitPlotCreate(plotId: string, title: string) {
    this.write("plot_create", { plotId, title });
  }

  emitOptions(options: DialogueOption[]) {
    this.write("options", { options });
  }

  emitStreamingMessages(messages: { speaker: string; type: string; text: string }[]) {
    this.write("streaming_messages", { messages });
  }

  emitParsed(messages: { speaker: string; type: string; text: string }[], options: DialogueOption[] | null) {
    this.write("parsed", { messages, options });
  }

  emitError(message: string) {
    this.write("error", { message });
  }
}

const KNOWN_TYPES = new Set(["YOU", "INNER_VOICE", "CHARACTER", "SYSTEM", "ROLL", "NOTIFICATION"]);
const VOICE_NAMES = new Set(["LOGIC", "RHETORIC", "VOLITION", "INLAND EMPIRE", "HALF LIGHT", "ELECTROCHEMISTRY", "SUGGESTION", "CONCEPTUALIZATION", "EMPATHY", "VISUAL CALCULUS"]);

interface ParsedHeader { speaker: string; type: string; bodyStart: number }

function parseHeader(trimmed: string): ParsedHeader | null {
  // Match [A|B] header at start of block — allow spaces, letters, underscores
  const hdr = trimmed.match(/^\[([^\]]+)\]\s*\n?/);
  if (!hdr) return null;

  const inner = hdr[1].trim();
  const pipeIdx = inner.indexOf("|");

  let speaker: string;
  let type: string;

  if (pipeIdx >= 0) {
    const a = inner.slice(0, pipeIdx).trim();
    const b = inner.slice(pipeIdx + 1).trim();

    // If A is a known type and B is not → reversed: [TYPE|SPEAKER]
    if (KNOWN_TYPES.has(a.toUpperCase()) && !KNOWN_TYPES.has(b.toUpperCase())) {
      type = a.toUpperCase();
      speaker = b;
    } else if (KNOWN_TYPES.has(b.toUpperCase()) && !KNOWN_TYPES.has(a.toUpperCase())) {
      // Standard: [SPEAKER|TYPE]
      speaker = a;
      type = b.toUpperCase();
    } else {
      // Both or neither are known types — default to [SPEAKER|TYPE]
      speaker = a;
      type = b.toUpperCase();
    }
  } else {
    // No pipe — single value
    const upper = inner.toUpperCase();
    if (KNOWN_TYPES.has(upper)) {
      // [SYSTEM], [INNER_VOICE], etc.
      type = upper;
      speaker = type === "SYSTEM" ? "NARRATOR" : inner;
    } else if (VOICE_NAMES.has(upper)) {
      // [LOGIC], [HALF LIGHT], etc. — inner voice speaking
      speaker = inner;
      type = "INNER_VOICE";
    } else {
      // [Madam Vespera], [Guard Captain], etc. — named character speaking
      speaker = inner;
      type = "CHARACTER";
    }
  }

  // Normalize: if speaker looks like a voice name, set type to INNER_VOICE
  if (VOICE_NAMES.has(speaker.toUpperCase()) || /^(LOGIC|RHETORIC|VOLITION|INLAND EMPIRE|HALF LIGHT|ELECTROCHEMISTRY|SUGGESTION|CONCEPTUALIZATION|EMPATHY|VISUAL CALCULUS)$/i.test(speaker)) {
    type = "INNER_VOICE";
  }

  // NARRATOR is always SYSTEM, never CHARACTER or INNER_VOICE
  if (speaker.toUpperCase() === "NARRATOR") {
    type = "SYSTEM";
  }

  return { speaker, type, bodyStart: hdr[0].length };
}

/** Parse the GM's text output into structured messages and options. */
export function parseResponseText(text: string): {
  messages: { speaker: string; type: string; text: string }[];
  options: DialogueOption[] | null;
} {
  const messages: { speaker: string; type: string; text: string }[] = [];

  // Extract <OPTIONS> block
  let options: DialogueOption[] | null = null;
  const optionsMatch = text.match(/<OPTIONS>\s*\n([\s\S]*?)\n\s*<\/OPTIONS>/);
  let cleanText = text;

  if (optionsMatch) {
    try {
      options = JSON.parse(optionsMatch[1]);
    } catch {
      const lines = optionsMatch[1].split("\n").filter(Boolean);
      options = lines.map((line, i) => ({
        id: `opt_${i}`,
        text: line.replace(/^-\s*/, "").trim(),
        isAiTrigger: true,
      }));
    }
    cleanText = text.replace(optionsMatch[0], "").trim();
  }

  // Parse message blocks separated by ---
  const blocks = cleanText.split(/\n?---\n?/);
  for (const block of blocks) {
    const trimmed = block.trim();
    if (!trimmed) continue;

    const parsed = parseHeader(trimmed);
    if (parsed) {
      let body = trimmed.slice(parsed.bodyStart).trim();
      // Strip (#entity_id) or (#internal) annotations the GM adds as references
      body = body.replace(/^\(#[^)]+\)\s*\n?/, '');
      if (body) messages.push({ speaker: parsed.speaker, type: parsed.type, text: body });
    } else {
      messages.push({ speaker: "NARRATOR", type: "SYSTEM", text: trimmed });
    }
  }

  return { messages, options };
}
