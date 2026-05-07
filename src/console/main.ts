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

import readline from "node:readline";
import chalk from "chalk";
import logUpdate from "log-update";
import type { Message, DialogueOption } from "@/types/dialogue";
import type { StreamingMessage } from "@/shared/events";
import { ConsoleSseClient, type SseCallbacks } from "./SseClient";

// ── Constants ──

const VOICE_COLORS: Record<string, string> = {
  LOGIC: "#4fb0c6",
  RHETORIC: "#c6b050",
  EMPATHY: "#c67080",
  PERCEPTION: "#50c6a0",
  VOLITION: "#e07840",
  ENDURANCE: "#c05050",
  SORCERY: "#9081e3",
  SUGGESTION: "#a0c650",
  INSTINCT: "#e05858",
  MIGHT: "#50c060",
  CLOCKWORK: "#50b0c6",
  ALCHEMY: "#9eff9e",
};

// ── State ──

type GameState = "IDLE" | "WAITING" | "AWAITING_OPTION";

let state: GameState = "IDLE";
let history: Message[] = [];
let currentOptions: DialogueOption[] = [];
let lastStepId: string | null = null;
let streamingMessages: Message[] = [];
let isRetrying = false;
let sseClient: ConsoleSseClient | null = null;
let messageIdCounter = 0;

const BASE_URL = process.env.ELYSSIAN_URL ?? "http://localhost:3000";

// ── Color Helpers ──

function hashNpcColor(name: string): number[] {
  let h = 5381;
  for (let i = 0; i < name.length; i++) {
    h = ((h << 5) + h) ^ name.charCodeAt(i);
  }
  const hue = Math.abs(h) % 360;
  return hslToRgb(hue, 48, 66);
}

function hslToRgb(h: number, s: number, l: number): [number, number, number] {
  s /= 100;
  l /= 100;
  const a = s * Math.min(l, 1 - l);
  const f = (n: number) => {
    const k = (n + h / 30) % 12;
    return l - a * Math.max(Math.min(k - 3, 9 - k, 1), -1);
  };
  return [Math.round(f(0) * 255), Math.round(f(8) * 255), Math.round(f(4) * 255)];
}

function getSpeakerColor(speaker: string, type: string) {
  if (type === "YOU") return chalk.hex("#d8d8d8");
  if (type === "INNER_VOICE") return chalk.hex(VOICE_COLORS[speaker.toUpperCase()] ?? "#9081e3");
  if (type === "SYSTEM" || type === "NOTIFICATION") return chalk.hex("#6b7280");
  if (type === "ROLL") return chalk.hex("#a3c2a3");
  const [r, g, b] = hashNpcColor(speaker);
  return chalk.rgb(r, g, b);
}

function stripOptionText(text: string): string {
  return text.replace(/^\[[^\]]*?:[^\]]*?\]\s*/, "");
}

// ── Rendering ──

function formatMessage(msg: Message | StreamingMessage, indent = 0, showCursor = false): string {
  let output = "";
  const prefix = " ".repeat(indent);
  const speakerColor = getSpeakerColor(msg.speaker, msg.type);
  const speakerName = msg.speaker === msg.type ? msg.type : `${msg.speaker}`;
  const displayName = msg.type === "CHARACTER" ? msg.speaker : speakerName;

  // Check for roll result display
  if ("rollResult" in msg && msg.rollResult) {
    const rr = msg.rollResult;
    const result = rr.success ? chalk.green("SUCCESS") : chalk.red("FAILURE");
    output +=
      prefix +
      speakerColor(`${displayName}`) +
      chalk.dim(`  [${rr.skill} ${rr.total} vs ${rr.difficulty}] `) +
      result +
      "\n";
    return output;
  }

  if (msg.type === "ROLL") {
    output += prefix + chalk.dim(`${msg.text}`) + "\n";
    return output;
  }

  output += prefix + speakerColor(displayName) + "\n";

  const textLines = msg.text.split("\n");
  for (let i = 0; i < textLines.length; i++) {
    const line = textLines[i];
    const isLastLine = i === textLines.length - 1;
    if (line.trim() !== "" || textLines.length === 1) {
      const cursor =
        showCursor && isLastLine ? chalk.hex("#ff6b35")("▌") : "";
      output += prefix + "  " + line + cursor + "\n";
    } else {
      output += "\n";
    }
  }
  return output;
}

function formatMessages(msgs: (Message | StreamingMessage)[]): string {
  return msgs.map((msg) => formatMessage(msg)).join("");
}

function formatStreamingMessages(): string {
  if (streamingMessages.length === 0) {
    return chalk.dim("  Generating story...\n");
  }
  let output = "";
  for (let i = 0; i < streamingMessages.length; i++) {
    const isLast = i === streamingMessages.length - 1;
    output += formatMessage(streamingMessages[i], 0, isLast);
  }
  return output;
}

function renderOptions(opts: DialogueOption[]) {
  const separator = "─".repeat(50);
  process.stdout.write(chalk.dim(`\n${separator}\n`));
  for (let i = 0; i < opts.length; i++) {
    const opt = opts[i];
    const num = chalk.hex("#ff6b35")(`${i + 1}.`);

    if (opt.check) {
      const checkColor = opt.check.isRed ? chalk.hex("#d34b34") : chalk.hex("#4fb0c6");
      process.stdout.write(
        `  ${num} ${checkColor(`[${opt.check.skill} - ${opt.check.difficultyText}]`)} ${opt.text}\n`,
      );
    } else {
      process.stdout.write(`  ${num} ${opt.text}\n`);
    }
  }
  process.stdout.write(chalk.dim(`${separator}\n`));
}

function renderBanner() {
  console.log("");
  console.log(chalk.bold("                  ELYSIAN DIALOGUE                  "));
  console.log(chalk.dim("               A Narrative RPG Engine               "));
  console.log("");
}

// ── SSE Callbacks ──

function createSseCallbacks(): SseCallbacks {
  return {
    onStepStart: (data) => {
      lastStepId = data.stepId;
    },
    onStreamingMessages: (messages) => {
      if (isRetrying) return;
      streamingMessages = messages.map((m, i) => ({
        id: `stream-${i}`,
        speaker: m.speaker,
        type: m.type as Message["type"],
        text: m.text,
        metadata: m.metadata as Message["metadata"],
      }));
      logUpdate(formatStreamingMessages());
    },
    onStreamingReset: () => {
      isRetrying = true;
    },
    onOptions: (options) => {
      currentOptions = options;
    },
    onParsed: (data) => {
      isRetrying = false;
      logUpdate.clear();
      logUpdate.done();
      streamingMessages = [];

      const messages: Message[] = data.messages.map((m) => ({
        id: `console-${messageIdCounter++}`,
        speaker: m.speaker,
        type: m.type as Message["type"],
        text: m.text,
        metadata: m.metadata as Message["metadata"],
      }));

      process.stdout.write(formatMessages(messages));
      history.push(...messages);
      console.log(""); // blank line before options

      if (data.options && data.options.length > 0) {
        currentOptions = data.options;
        renderOptions(data.options);
        state = "AWAITING_OPTION";
      } else {
        currentOptions = [];
        console.log(chalk.dim("(No choices available)"));
        state = "IDLE";
      }

      if (state === "AWAITING_OPTION") {
        showPrompt();
      } else {
        showBeginPrompt();
      }
    },
    onError: (message) => {
      isRetrying = false;
      logUpdate.clear();
      logUpdate.done();
      streamingMessages = [];
      console.log(chalk.red(`\n[ERROR] ${message}\n`));
      if (currentOptions.length > 0) {
        state = "AWAITING_OPTION";
        showPrompt();
      } else {
        state = "IDLE";
        showBeginPrompt();
      }
    },
    onDone: () => {
      // parsed already transitioned state
    },
  };
}

// ── API Calls ──

async function postChatStream(
  userInput: string,
  hist: Message[],
  parentStepId: string | null,
  parentOptionId: string | null,
) {
  state = "WAITING";
  isRetrying = false;
  streamingMessages = [];
  currentOptions = [];
  sseClient?.abort();

  const client = new ConsoleSseClient();
  sseClient = client;

  await client.stream(
    `${BASE_URL}/api/chat/stream`,
    {
      userInput,
      history: hist,
      parentStepId,
      parentOptionId,
      playerCharacter: null,
    },
    createSseCallbacks(),
  );
}

async function postRegenerate(stepId: string, hist: Message[]) {
  state = "WAITING";
  isRetrying = false;
  streamingMessages = [];
  currentOptions = [];
  sseClient?.abort();

  const client = new ConsoleSseClient();
  sseClient = client;

  await client.stream(
    `${BASE_URL}/api/regenerate`,
    {
      stepId,
      history: hist,
      playerCharacter: null,
    },
    createSseCallbacks(),
  );
}

// ── Game Actions ──

async function handleBegin() {
  console.log(chalk.dim("Starting story...\n"));
  await postChatStream("[SYSTEM MESSAGE: Begin the story. Set the scene.]", [], null, null);
}

async function handleOptionSelect(option: DialogueOption) {
  const youText = option.selectionMessage ?? stripOptionText(option.text);

  const youMessage: Message = {
    id: `console-${messageIdCounter++}`,
    speaker: "YOU",
    type: "YOU",
    text: youText,
  };
  history = [...history, youMessage];

  process.stdout.write("\n");
  process.stdout.write(formatMessage(youMessage));
  console.log("");

  await postChatStream(youText, history, lastStepId, option.id);
}

async function handleRegenerate() {
  if (!lastStepId) return;
  sseClient?.abort();

  const lastYouIdx = history.map((m) => m.type).lastIndexOf("YOU");
  const trimmedHistory = lastYouIdx >= 0 ? history.slice(0, lastYouIdx + 1) : history;

  console.log(chalk.dim("\nRegenerating...\n"));
  history = trimmedHistory;

  await postRegenerate(lastStepId, trimmedHistory);
}

// ── Resume ──

async function tryResume(): Promise<boolean> {
  try {
    const histRes = await fetch(`${BASE_URL}/api/history`);
    if (!histRes.ok) return false;
    const hist: Message[] = await histRes.json();
    if (hist.length === 0) return false;

    const currRes = await fetch(`${BASE_URL}/api/session/current`);
    if (!currRes.ok) return false;
    const current = await currRes.json();
    if (!current || !current.options || current.options.length === 0) return false;

    history = hist;
    lastStepId = current.id;
    currentOptions = current.options;
    messageIdCounter = hist.length;

    console.log(chalk.dim("\n╌╌╌ Resuming session ╌╌╌\n"));
    process.stdout.write(formatMessages(hist));
    console.log("");
    renderOptions(current.options);

    state = "AWAITING_OPTION";
    return true;
  } catch {
    return false;
  }
}

// ── Prompt Helpers ──

function showPrompt() {
  process.stdout.write(
    chalk.hex("#ff6b35")("> ") +
      chalk.dim("[1-") +
      chalk.hex("#ff6b35")(`${currentOptions.length}`) +
      chalk.dim("] choose  ") +
      chalk.dim("[r]egenerate  [q]uit\n"),
  );
}

function showBeginPrompt() {
  process.stdout.write(chalk.dim("\nPress ENTER to begin your story...\n"));
}

// ── Main ──

async function main() {
  console.clear();
  renderBanner();

  const rl = readline.createInterface({
    input: process.stdin,
    output: process.stdout,
  });

  // Graceful shutdown
  process.on("SIGINT", () => {
    sseClient?.abort();
    console.log(chalk.dim("\n\nFarewell.\n"));
    rl.close();
    process.exit(0);
  });

  // Check for existing session
  const resumed = await tryResume();

  if (!resumed) {
    console.log(chalk.dim("Press ENTER to begin your story..."));
  }

  // Main input loop
  for await (const line of rl) {
    const input = line.trim().toLowerCase();

    if (state === "IDLE") {
      if (input === "" || input === "b" || input === "begin") {
        await handleBegin();
      } else if (input === "q") {
        break;
      }
    } else if (state === "AWAITING_OPTION") {
      if (input === "r") {
        await handleRegenerate();
      } else if (input === "q") {
        break;
      } else {
        const idx = parseInt(input, 10);
        if (isNaN(idx) || idx < 1 || idx > currentOptions.length) {
          console.log(
            chalk.yellow(
              `Invalid choice. Pick 1-${currentOptions.length}, 'r' to regenerate, or 'q' to quit.`,
            ),
          );
          showPrompt();
        } else {
          await handleOptionSelect(currentOptions[idx - 1]);
        }
      }
    }
    // WAITING state: input is ignored, SSE callbacks drive transitions
  }

  sseClient?.abort();
  console.log(chalk.dim("\n" + "Farewell." + "\n"));
  rl.close();
  process.exit(0);
}

main();
