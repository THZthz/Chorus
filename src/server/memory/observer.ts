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
 * Observational Memory - context compression and observation extraction.
 *
 * The MemoryObserver monitors accumulated context per session and extracts
 * high-level observations (key facts, decisions, topic shifts) when the
 * token count exceeds a configurable threshold.
 *
 * This implements the three-tier context hierarchy:
 * 1. Reflections - high-level session summaries (generated when threshold exceeded)
 * 2. Observations - extracted facts, decisions, preferences from messages
 * 3. Recent messages - the most recent messages in the session
 *
 * Takes ShortTermMemory directly (not MemoryClient) to avoid circular imports.
 * Falls back to keyword/entity-based extraction when no LLM is available.
 */

import type { ShortTermMemory } from "./short-term";
import type { Observation, ObservationResult } from "./types";

const CHARS_PER_TOKEN = 4;

interface SessionContext {
  sessionId: string;
  totalChars: number;
  messageCount: number;
  observations: Observation[];
  reflections: string[];
  lastCompressionAt: number;
  entityNames: Set<string>;
  topics: string[];
}

const DECISION_MARKERS = [
  "i decided",
  "i've decided",
  "let's go with",
  "i'll go with",
  "i chose",
  "i've chosen",
  "we should",
  "i want to",
  "i'm going to",
  "i plan to",
];

const FACT_PATTERNS = [
  "the answer is",
  "it turns out",
  "actually,",
  "i found out",
  "i learned that",
  "it seems like",
  "the reason is",
];

function extractSentenceContaining(text: string, marker: string): string | null {
  const lower = text.toLowerCase();
  const idx = lower.indexOf(marker.toLowerCase());
  if (idx === -1) return null;

  // Find sentence start (look backward for sentence boundary)
  let start = Math.max(0, idx);
  for (let i = idx - 1; i >= 0; i--) {
    if (".!?\n".includes(text[i])) {
      start = i + 1;
      break;
    }
  }

  // Find sentence end (look forward for sentence boundary)
  let end = text.length;
  for (let i = idx + marker.length; i < text.length; i++) {
    if (".!?\n".includes(text[i])) {
      end = i + 1;
      break;
    }
  }

  const sentence = text.slice(start, end).trim();

  // Cap at reasonable length
  if (sentence.length > 300) {
    const truncated = sentence.slice(0, 300);
    const lastSpace = truncated.lastIndexOf(" ");
    return lastSpace > 0 ? truncated.slice(0, lastSpace) + "..." : truncated + "...";
  }

  return sentence.length > 10 ? sentence : null;
}

export class MemoryObserver {
  private shortTerm: ShortTermMemory;
  private thresholdTokens: number;
  private recentWindow: number;
  private sessions = new Map<string, SessionContext>();

  constructor(
    shortTerm: ShortTermMemory,
    options?: { thresholdTokens?: number; recentMessageWindow?: number },
  ) {
    this.shortTerm = shortTerm;
    this.thresholdTokens = options?.thresholdTokens ?? 30000;
    this.recentWindow = options?.recentMessageWindow ?? 20;
  }

  async onMessageStored(
    sessionId: string,
    content: string,
    messageId?: string,
    role: string = "user",
  ): Promise<void> {
    const ctx = this.getSession(sessionId);
    ctx.totalChars += content.length;
    ctx.messageCount += 1;

    // Extract inline observations from user messages
    if (role === "user") {
      const observations = this.extractInlineObservations(content, messageId);
      ctx.observations.push(...observations);
    }

    // Check if we need to compress
    const approxTokens = Math.floor(ctx.totalChars / CHARS_PER_TOKEN);
    if (approxTokens > this.thresholdTokens) {
      const msgsSinceCompression = ctx.messageCount - ctx.lastCompressionAt;
      if (msgsSinceCompression >= this.recentWindow) {
        await this.generateReflection(sessionId);
      }
    }
  }

  async getObservations(sessionId: string): Promise<ObservationResult> {
    const ctx = this.getSession(sessionId);
    return {
      sessionId,
      messageCount: ctx.messageCount,
      approximateTokens: Math.floor(ctx.totalChars / CHARS_PER_TOKEN),
      thresholdTokens: this.thresholdTokens,
      thresholdExceeded: Math.floor(ctx.totalChars / CHARS_PER_TOKEN) > this.thresholdTokens,
      reflections: ctx.reflections,
      observations: ctx.observations.map((o) => ({
        type: o.type,
        content: o.content,
        confidence: o.confidence,
        timestamp: o.timestamp,
        sourceMessageId: o.sourceMessageId,
      })),
      entityNames: Array.from(ctx.entityNames).sort(),
      topics: ctx.topics,
    };
  }

  resetSession(sessionId: string): void {
    this.sessions.delete(sessionId);
  }

  // ── Private helpers ──

  private getSession(sessionId: string): SessionContext {
    if (!this.sessions.has(sessionId)) {
      this.sessions.set(sessionId, {
        sessionId,
        totalChars: 0,
        messageCount: 0,
        observations: [],
        reflections: [],
        lastCompressionAt: 0,
        entityNames: new Set(),
        topics: [],
      });
    }
    return this.sessions.get(sessionId)!;
  }

  private async generateReflection(sessionId: string): Promise<void> {
    const ctx = this.getSession(sessionId);
    try {
      // Get conversation messages (up to 100)
      const messages = await this.shortTerm.getConversation(sessionId, 100);
      if (messages.length === 0) return;

      // Focus on older messages (beyond the recent window)
      const olderMessages = messages.slice(0, -this.recentWindow);
      if (olderMessages.length === 0) return;

      // Extract capitalized multi-word phrases as potential entities
      const entities = new Set<string>();
      for (const msg of olderMessages) {
        const words = msg.content.split(/\s+/);
        for (let i = 0; i < words.length; i++) {
          const word = words[i];
          if (word && word[0] === word[0].toUpperCase() && word.length > 2) {
            const parts = [word];
            for (let j = i + 1; j < Math.min(i + 4, words.length); j++) {
              if (words[j] && words[j][0] === words[j][0].toUpperCase()) {
                parts.push(words[j]);
              } else {
                break;
              }
            }
            if (parts.length > 1) {
              entities.add(parts.join(" "));
            }
          }
        }
      }

      // Create a reflection from accumulated data
      const reflectionParts: string[] = [];
      if (ctx.observations.length > 0) {
        const obsSummary = ctx.observations
          .slice(-10)
          .map((o) => o.content)
          .join("; ");
        reflectionParts.push(`Key observations: ${obsSummary}`);
      }
      if (entities.size > 0) {
        const topEntities = Array.from(entities).sort().slice(0, 10);
        reflectionParts.push(`Entities discussed: ${topEntities.join(", ")}`);
      }
      if (reflectionParts.length > 0) {
        ctx.reflections.push(
          `Session summary (${ctx.messageCount} messages): ` + reflectionParts.join(". "),
        );
      }

      ctx.lastCompressionAt = ctx.messageCount;
    } catch {
      // Silently skip compression on error
    }
  }

  private extractInlineObservations(content: string, messageId?: string): Observation[] {
    const observations: Observation[] = [];
    const now = new Date().toISOString();

    // Look for decision/action statements
    for (const marker of DECISION_MARKERS) {
      if (content.toLowerCase().includes(marker)) {
        const sentence = extractSentenceContaining(content, marker);
        if (sentence) {
          observations.push({
            type: "decision",
            content: sentence,
            sourceMessageId: messageId,
            timestamp: now,
            confidence: 0.75,
          });
        }
        break; // One decision per message
      }
    }

    // Look for factual statements
    for (const marker of FACT_PATTERNS) {
      if (content.toLowerCase().includes(marker)) {
        const sentence = extractSentenceContaining(content, marker);
        if (sentence) {
          observations.push({
            type: "fact",
            content: sentence,
            sourceMessageId: messageId,
            timestamp: now,
            confidence: 0.7,
          });
        }
        break; // One fact per message
      }
    }

    return observations;
  }
}
