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

import { select, input, Separator } from "@inquirer/prompts";
import chalk from "chalk";
import logUpdate from "log-update";
import wrapAnsi from "wrap-ansi";
import { renderMarkdown } from "@/console/markdown";
import type { Message, DialogueOption } from "@/types/dialogue";
import type { StreamingMessage } from "@/server/llm/events";
import { ConsoleSseClient, type SseCallbacks } from "@/console/SseClient";
import { VOICE_COLORS } from "@/shared/colors.ts";

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

const BASE_URL = process.env.ELYSIAN_URL ?? "http://localhost:3000";

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
    output += prefix + chalk.dim(`${msg.text ?? ""}`) + "\n";
    return output;
  }

  output += prefix + speakerColor(displayName) + "\n";

  const termWidth = process.stdout.columns ?? 80;
  const textWidth = Math.max(40, termWidth - indent - 2);

  const rendered = renderMarkdown(msg.text);
  const wrapped = wrapAnsi(rendered, textWidth, { hard: true });
  const wrappedLines = wrapped.split("\n");
  for (let i = 0; i < wrappedLines.length; i++) {
    const line = wrappedLines[i];
    const isLastLine = i === wrappedLines.length - 1;
    const cursor = showCursor && isLastLine ? chalk.hex("#ff6b35")("▌") : "";
    output += prefix + "  " + line + cursor + "\n";
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

function formatOptionLabel(opt: DialogueOption): string {
  if (opt.check) {
    const checkColor = opt.check.isRed ? chalk.hex("#d34b34") : chalk.hex("#4fb0c6");
    return `${checkColor(`[${opt.check.skill} - ${opt.check.difficultyText}]`)} ${opt.text}`;
  }
  return opt.text;
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
      currentOptions = options as unknown as DialogueOption[];
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
        currentOptions = data.options as unknown as DialogueOption[];
        state = "AWAITING_OPTION";
      } else {
        currentOptions = [];
        console.log(chalk.dim("(No choices available)"));
        state = "IDLE";
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
      } else {
        state = "IDLE";
      }
    },
    onDone: () => {
      // parsed already transitioned state
    },
  };
}

// ── API Calls ──

async function postChatStream(userInput: string, hist: Message[]) {
  state = "WAITING";
  isRetrying = false;
  streamingMessages = [];
  currentOptions = [];
  sseClient?.abort();

  const client = new ConsoleSseClient();
  sseClient = client;

  await client.stream(
    `${BASE_URL}/api/chat/stream`,
    { userInput, history: hist },
    createSseCallbacks(),
  );
}

// ── Game Actions ──

async function handleBegin() {
  console.log(chalk.dim("Starting story...\n"));
  await postChatStream("[SYSTEM MESSAGE: Begin the story. Set the scene.]", []);
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

  await postChatStream(youText, history);
}

// ── Resume ──

async function checkResumable(): Promise<boolean> {
  try {
    const currRes = await fetch(`${BASE_URL}/api/game/current`);
    if (!currRes.ok) return false;
    const current = (await currRes.json()) as { id: string; options: DialogueOption[] };
    return !!(current && current.options && current.options.length > 0);
  } catch {
    return false;
  }
}

async function doResume(): Promise<boolean> {
  try {
    const histRes = await fetch(`${BASE_URL}/api/history`);
    if (!histRes.ok) {
      console.error(`[resume] history fetch failed: ${histRes.status} ${histRes.statusText}`);
      return false;
    }
    const hist = (await histRes.json()) as Message[];
    if (hist.length === 0) {
      console.error("[resume] history is empty");
      return false;
    }

    const currRes = await fetch(`${BASE_URL}/api/game/current`);
    if (!currRes.ok) {
      console.error(`[resume] game/current fetch failed: ${currRes.status} ${currRes.statusText}`);
      return false;
    }
    const current = (await currRes.json()) as { id: string; options: DialogueOption[] };
    if (!current || !current.options || current.options.length === 0) {
      console.error("[resume] no current options available");
      return false;
    }

    history = hist;
    lastStepId = current.id;
    currentOptions = current.options;
    messageIdCounter = hist.length;

    console.log(chalk.dim("\n╌╌╌ Resuming story ╌╌╌\n"));
    process.stdout.write(formatMessages(hist));
    console.log("");

    state = "AWAITING_OPTION";
    return true;
  } catch (err) {
    console.error("[resume] unexpected error:", err);
    return false;
  }
}

// ── Prompt Helpers ──

async function presentChoice(
  options: DialogueOption[],
): Promise<number | "custom" | "reset" | "help" | "quit"> {
  const sep = "─".repeat(50);
  console.log(chalk.dim(sep));

  const choices: Array<
    | { name: string; value: number | "custom" | "reset" | "help" | "quit"; description?: string }
    | InstanceType<typeof Separator>
  > = [
    ...options.map((opt, i) => ({
      name: formatOptionLabel(opt),
      value: i as number,
      description: opt.check
        ? `${opt.check.isRed ? "RED CHECK — one-time only. " : ""}Roll 2D6 + ${opt.check.skill} vs ${opt.check.difficultyText}`
        : undefined,
    })),
    new Separator(chalk.dim(sep)),
    { name: chalk.hex("#ff6b35")("[Custom input...]"), value: "custom" as const },
    { name: chalk.dim("/reset  Clear and restart"), value: "reset" as const },
    { name: chalk.dim("/help   Show available commands"), value: "help" as const },
    { name: "Quit", value: "quit" as const },
  ];

  return await select<number | "custom" | "reset" | "help" | "quit">({
    message: "Choose your action:",
    choices,
    pageSize: Math.min(options.length + 5, 12),
    loop: false,
  });
}

// ── Main ──

async function main() {
  console.clear();
  renderBanner();

  // Graceful shutdown
  process.on("SIGINT", () => {
    sseClient?.abort();
    console.log(chalk.dim("\n\nFarewell.\n"));
    process.exit(0);
  });

  // Check if a resumable game exists
  let resumable = await checkResumable();

  // Main loop
  while (true) {
    if (state === "IDLE") {
      const idleChoices: Array<{ name: string; value: string }> = [];
      if (resumable) {
        idleChoices.push({ name: chalk.hex("#4fb0c6")("Resume your story"), value: "resume" });
      }
      idleChoices.push({ name: "Begin your story", value: "begin" });
      idleChoices.push({ name: "/help   Show available commands", value: "help" });
      idleChoices.push({ name: "Quit", value: "quit" });

      const answer = await select({
        message: "What would you like to do?",
        choices: idleChoices,
        loop: false,
      });

      if (answer === "resume") {
        const ok = await doResume();
        if (!ok) {
          resumable = false;
          console.log(chalk.red("\nFailed to resume. Starting a new game instead.\n"));
        }
      } else if (answer === "begin") {
        await handleBegin();
      } else if (answer === "help") {
        showHelp();
      } else {
        break;
      }
    } else if (state === "AWAITING_OPTION") {
      const choice = await presentChoice(currentOptions);

      if (choice === "custom") {
        const customText = await input({ message: "What do you want to do or say?" });
        if (!customText.trim()) continue;

        const youMessage: Message = {
          id: `console-${messageIdCounter++}`,
          speaker: "YOU",
          type: "YOU",
          text: customText.trim(),
        };
        history = [...history, youMessage];

        process.stdout.write("\n");
        process.stdout.write(formatMessage(youMessage));
        console.log("");

        await postChatStream(customText.trim(), history);
      } else if (choice === "reset") {
        const answer = await select({
          message: "Reset will clear the session. Are you sure?",
          choices: [
            { name: "Yes, reset", value: "yes" },
            { name: "Cancel", value: "cancel" },
          ],
          loop: false,
        });
        if (answer === "yes") {
          state = "IDLE";
          history = [];
          currentOptions = [];
          streamingMessages = [];
          lastStepId = null;
          messageIdCounter = 0;
          sseClient?.abort();
          console.log(chalk.dim("\nSession reset.\n"));
        }
      } else if (choice === "help") {
        showHelp();
      } else if (choice === "quit") {
        break;
      } else {
        await handleOptionSelect(currentOptions[choice]);
      }
    } else {
      // WAITING state: pause briefly to avoid busy-waiting
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
  }

  sseClient?.abort();
  console.log(chalk.dim("\n" + "Farewell." + "\n"));
  process.exit(0);
}

function showHelp() {
  console.log("");
  console.log(chalk.bold("Available commands:"));
  console.log("");
  console.log(chalk.dim("  Type text  ") + "  Send input and generate a response");
  console.log(chalk.dim("  /reset     ") + "  Clear the session and restart");
  console.log(chalk.dim("  /help      ") + "  Show this help message");
  console.log(chalk.dim("  /exit      ") + "  Quit the console client");
  console.log("");
}

main();
