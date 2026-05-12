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

/**
 * Observational Memory — tracks world state changes and compresses old context.
 *
 * Hooks into updateWorld tool execution to record structured deltas
 * (location moves, entity changes, relationships, facts) rather than
 * scanning message text for keywords.
 *
 * Maintains token-threshold compression for long sessions:
 * when accumulated context exceeds the threshold, generates reflections
 * from older messages to keep the GM's context window manageable.
 */

import type { ShortTermMemory } from "@/server/memory/shortTerm";
import type { Observation, ObservationResult } from "@/server/memory/types";

const CHARS_PER_TOKEN = 4;

interface WorldDelta {
  action: string;
  summary: string;
  timestamp: string;
}

interface ObserverContext {
  totalChars: number;
  messageCount: number;
  deltas: WorldDelta[];
  reflections: string[];
  lastCompressionAt: number;
}

export class MemoryObserver {
  private shortTerm: ShortTermMemory;
  private thresholdTokens: number;
  private recentWindow: number;
  private ctx: ObserverContext | null = null;

  constructor(
    shortTerm: ShortTermMemory,
    options?: { thresholdTokens?: number; recentMessageWindow?: number },
  ) {
    this.shortTerm = shortTerm;
    this.thresholdTokens = options?.thresholdTokens ?? 30_000;
    this.recentWindow = options?.recentMessageWindow ?? 20;
  }

  /** Called from updateWorld on each state mutation. */
  onWorldChange(delta: { action: string; summary: string }): void {
    const ctx = this.getContext();
    ctx.deltas.push({
      action: delta.action,
      summary: delta.summary,
      timestamp: new Date().toISOString(),
    });
  }

  /** Called when a message is stored (dialogue tracking for compression). */
  async onMessageStored(
    content: string,
    _messageId?: string,
    _role: string = "user",
  ): Promise<void> {
    const ctx = this.getContext();
    ctx.totalChars += content.length;
    ctx.messageCount += 1;

    const approxTokens = Math.floor(ctx.totalChars / CHARS_PER_TOKEN);
    if (approxTokens > this.thresholdTokens) {
      const msgsSinceCompression = ctx.messageCount - ctx.lastCompressionAt;
      if (msgsSinceCompression >= this.recentWindow) {
        await this.generateReflection();
      }
    }
  }

  async getObservations(): Promise<ObservationResult> {
    const ctx = this.getContext();
    const observations: Observation[] = ctx.deltas.slice(-20).map((d) => ({
      type: "fact" as const,
      content: `[${d.action}] ${d.summary}`,
      timestamp: d.timestamp,
      confidence: 1.0,
    }));

    return {
      messageCount: ctx.messageCount,
      approximateTokens: Math.floor(ctx.totalChars / CHARS_PER_TOKEN),
      thresholdTokens: this.thresholdTokens,
      thresholdExceeded: Math.floor(ctx.totalChars / CHARS_PER_TOKEN) > this.thresholdTokens,
      reflections: ctx.reflections,
      observations,
      entityNames: [],
      topics: [],
    };
  }

  reset(): void {
    this.ctx = null;
  }

  // ── Private ──

  private getContext(): ObserverContext {
    if (!this.ctx) {
      this.ctx = {
        totalChars: 0,
        messageCount: 0,
        deltas: [],
        reflections: [],
        lastCompressionAt: 0,
      };
    }
    return this.ctx;
  }

  private async generateReflection(): Promise<void> {
    const ctx = this.getContext();
    try {
      const messages = await this.shortTerm.getConversation(100);
      if (messages.length === 0) return;

      const olderMessages = messages.slice(0, -this.recentWindow);
      if (olderMessages.length === 0) return;

      const reflectionParts: string[] = [];

      // Summarize recent world deltas
      if (ctx.deltas.length > 0) {
        const recentDeltas = ctx.deltas.slice(-10);
        const deltaSummary = recentDeltas.map((d) => `[${d.action}] ${d.summary}`).join("; ");
        reflectionParts.push(`World changes: ${deltaSummary}`);
      }

      // Summarize older messages (first 100 chars each)
      const msgExcerpts = olderMessages.slice(-5).map((m) =>
        `${m.role}: ${m.content.slice(0, 100)}`
      );
      if (msgExcerpts.length > 0) {
        reflectionParts.push(`Earlier dialogue: ${msgExcerpts.join(" | ")}`);
      }

      if (reflectionParts.length > 0) {
        ctx.reflections.push(
          `Session reflection (${ctx.messageCount} messages): ` + reflectionParts.join(". "),
        );
      }

      ctx.lastCompressionAt = ctx.messageCount;
    } catch {
      // Silently skip compression on error
    }
  }
}
