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
const VOICE_NAMES = new Set([
  "LOGIC", "RHETORIC", "VOLITION", "INLAND EMPIRE", "HALF LIGHT", "ELECTROCHEMISTRY", 
  "SUGGESTION", "CONCEPTUALIZATION", "EMPATHY", "VISUAL CALCULUS", "PERCEPTION", 
  "ENDURANCE", "PHYSICAL INSTRUMENT", "DRAMA", "AUTHORITY", "ESPRIT DE CORPS", 
  "SHIVERS", "PAIN THRESHOLD", "HAND/EYE COORDINATION", "REACTION SPEED", 
  "SAVOIR FAIRE", "INTERPERSONAL", "COMPOSURE", "ENCYCLOPEDIA"
]);

interface ParsedHeader { speaker: string; type: string; bodyStart: number }

function parseHeader(trimmed: string): ParsedHeader | null {
  // Match [A|B] header at start of block — allow spaces, letters, underscores, slashes, dashes
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
  const speakerUpper = speaker.toUpperCase();
  const isInternalVoice = VOICE_NAMES.has(speakerUpper) || 
    /^(LOGIC|RHETORIC|VOLITION|INLAND EMPIRE|HALF LIGHT|ELECTROCHEMISTRY|SUGGESTION|CONCEPTUALIZATION|EMPATHY|VISUAL CALCULUS|PERCEPTION|ENDURANCE|PHYSICAL INSTRUMENT|DRAMA|AUTHORITY|ESPRIT DE CORPS|SHIVERS|PAIN THRESHOLD|HAND\/EYE COORDINATION|REACTION SPEED|SAVOIR FAIRE|INTERPERSONAL|COMPOSURE|ENCYCLOPEDIA)$/i.test(speaker);

  if (isInternalVoice) {
    type = "INNER_VOICE";
  }

  // NARRATOR is always SYSTEM, never CHARACTER or INNER_VOICE
  if (speakerUpper === "NARRATOR") {
    type = "SYSTEM";
    speaker = "NARRATOR";
  }

  return { speaker, type, bodyStart: hdr[0].length };
}

/** Parse the GM's text output into structured messages and options. */
export function parseResponseText(text: string): {
  messages: { speaker: string; type: string; text: string }[];
  options: DialogueOption[] | null;
} {
  const messages: { speaker: string; type: string; text: string }[] = [];

  // 1. Extract <OPTIONS> block
  let options: DialogueOption[] | null = null;
  const optionsMatch = text.match(/<OPTIONS>\s*\n([\s\S]*?)\n\s*<\/OPTIONS>/i);
  let cleanText = text;

  if (optionsMatch) {
    try {
      options = JSON.parse(optionsMatch[1]);
    } catch {
      // Fallback for non-JSON options
      const lines = optionsMatch[1].split("\n").filter(l => l.trim());
      options = lines.map((line, i) => ({
        id: `opt_${i}`,
        text: line.replace(/^-\s*/, "").trim(),
        isAiTrigger: true,
      }));
    }
    cleanText = text.replace(optionsMatch[0], "").trim();
  }

  // 2. Identify all [SPEAKER|TYPE] blocks. 
  // We use a regex that looks for these headers and splits the content.
  // We look for [Header] at start of lines.
  const headerRegex = /^\[([^\]]+)\]/gm;
  const parts: string[] = [];
  let lastIdx = 0;
  let match;

  // We want to split the text into blocks starting with [Header]
  while ((match = headerRegex.exec(cleanText)) !== null) {
     if (match.index > lastIdx) {
       parts.push(cleanText.slice(lastIdx, match.index));
     }
     lastIdx = match.index;
  }
  parts.push(cleanText.slice(lastIdx));

  for (const part of parts) {
    const trimmed = part.trim();
    if (!trimmed || trimmed === '---') continue;

    // Remove any trailing --- from the previous block if present
    const content = trimmed.replace(/\n?---\n?$/, '').trim();
    if (!content) continue;

    const parsed = parseHeader(content);
    if (parsed) {
      let body = content.slice(parsed.bodyStart).trim();
      // Strip any separator at the end of the body
      body = body.replace(/\n?---\n?$/, '').trim();
      // Strip (#entity_id) or (#internal) annotations
      body = body.replace(/^\(#[^)]+\)\s*\n?/, '');
      if (body) messages.push({ speaker: parsed.speaker, type: parsed.type, text: body });
    } else {
      // If it doesn't start with a header, it's either leading narration or nested narration
      messages.push({ speaker: "NARRATOR", type: "SYSTEM", text: content });
    }
  }

  return { messages, options };
}
