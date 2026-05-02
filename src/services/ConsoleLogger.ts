export type LogLevel = "trace" | "log" | "info" | "warn" | "error";

export interface ConsoleLog {
  id: string;
  level: LogLevel;
  message: string;
  timestamp: number;
  args: any[];
}

type LogListener = (log: ConsoleLog) => void;

const MAX_DEPTH = 3;
const MAX_KEYS = 20;

function safeSerialize(value: unknown, depth: number = 0): unknown {
  if (value === null || value === undefined) return value;
  if (typeof value === "string" || typeof value === "number" || typeof value === "boolean")
    return value;
  if (typeof value === "bigint") return `BigInt(${value})`;
  if (typeof value === "symbol") return value.toString();
  if (typeof value === "function") return `[Function: ${(value as (...args: unknown[]) => unknown).name || "anonymous"}]`;

  if (typeof value === "object") {
    if (depth >= MAX_DEPTH) return "[MaxDepth]";

    try {
      // DOM nodes
      if (value instanceof Node) return `[${value.nodeName}]`;

      if (Array.isArray(value)) {
        const slice = value.slice(0, MAX_KEYS);
        const result = slice.map((v) => safeSerialize(v, depth + 1));
        if (value.length > MAX_KEYS) result.push(`[...${value.length - MAX_KEYS} more]`);
        return result;
      }
      if (value instanceof Error) {
        return {
          name: value.name,
          message: value.message,
          stack: value.stack,
          __isError: true,
        };
      }
      const keys = Object.keys(value).slice(0, MAX_KEYS);
      const plain: Record<string, unknown> = {};
      for (const key of keys) {
        try {
          plain[key] = safeSerialize((value as Record<string, unknown>)[key], depth + 1);
        } catch {
          plain[key] = "[Unserializable]";
        }
      }
      if (Object.keys(value).length > MAX_KEYS) {
        plain["..."] = `[${Object.keys(value).length - MAX_KEYS} more keys]`;
      }
      return plain;
    } catch {
      return "[Unserializable]";
    }
  }

  return String(value);
}

function safeStringify(value: unknown): string {
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
}

class ConsoleLogger {
  private logs: ConsoleLog[] = [];
  private listeners: Set<LogListener> = new Set();
  private originalConsole: Partial<
    Record<LogLevel | "debug", (...args: any[]) => void>
  > = {};
  private batchQueue: ConsoleLog[] = [];
  private batchTimer: ReturnType<typeof setTimeout> | null = null;
  private readonly BATCH_MS = 500;

  constructor() {
    if (typeof window !== "undefined") {
      this.init();
    }
  }

  private scheduleBatchFlush() {
    if (this.batchTimer !== null) return;
    this.batchTimer = setTimeout(() => {
      this.flushBatch();
    }, this.BATCH_MS);
  }

  private async flushBatch() {
    const batch = this.batchQueue;
    this.batchQueue = [];
    this.batchTimer = null;

    if (batch.length === 0) return;

    try {
      await fetch("/api/debug/console", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: safeStringify(
          batch.map((log) => ({
            level: log.level,
            message: log.message,
            args: log.args,
          })),
        ),
      });
    } catch (err) {
      this.originalConsole.error?.("Failed to persist console logs:", err);
    }
  }

  private init() {
    const levels: (LogLevel | "debug")[] = [
      "trace",
      "debug",
      "log",
      "info",
      "warn",
      "error",
    ];

    levels.forEach((level) => {
      if (typeof console[level] === "function") {
        this.originalConsole[level] = console[level].bind(console);
      }

      console[level] = (...args: any[]) => {
        this.originalConsole[level]?.(...args);

        const safeArgs = args.map((arg) => safeSerialize(arg));
        const normalizedLevel: LogLevel =
          level === "debug" ? "trace" : level;

        const log: ConsoleLog = {
          id: Math.random().toString(36).substring(2, 9),
          level: normalizedLevel,
          message: safeArgs
            .map((arg) => {
              if (typeof arg === "string") return arg;
              return safeStringify(arg);
            })
            .join(" "),
          timestamp: Date.now(),
          args: safeArgs,
        };

        this.logs.push(log);
        if (this.logs.length > 1000) {
          this.logs.shift();
        }

        this.batchQueue.push(log);
        this.scheduleBatchFlush();

        this.listeners.forEach((listener) => listener(log));
      };
    });
  }

  getLogs() {
    return [...this.logs];
  }

  setLogs(logs: ConsoleLog[]) {
    this.logs = logs;
    this.listeners.forEach((listener) => listener({} as ConsoleLog));
  }

  async clearLogs() {
    this.logs = [];
    try {
      await fetch("/api/debug/console/clear", { method: "POST" });
    } catch (err) {
      this.originalConsole.error?.("Failed to clear persisted console logs:", err);
    }
    this.listeners.forEach((listener) => listener({} as ConsoleLog));
  }

  subscribe(listener: LogListener) {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }
}

export const consoleLogger = new ConsoleLogger();
