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

import { tool } from "ai";
import { z } from "zod";
import { advanceGameTime, describeTime } from "@/server/models/scene";
import type { EventEmitter } from "@/server/llm/events";
import { TOOL_NAMES } from "@/shared/constants";
import { wrapSafe } from "@/server/llm/tools/shared";

const inputSchema = z.object({
  segments: z
    .number()
    .int()
    .min(0)
    .max(11)
    .optional()
    .describe("Number of 2-hour segments to advance (0-11)."),
  days: z
    .number()
    .int()
    .min(0)
    .optional()
    .describe("Number of full days to advance (0+), for multi-day travel or long activities."),
  reason: z
    .string()
    .optional()
    .describe("Brief narrative reason for the time advance (e.g. 'The conversation dragged on')."),
});

export function createAdvanceTimeTool(events: EventEmitter) {
  return tool({
    title: "Advance Time",
    description:
      "Advance the in-game clock. Use `segments` (0-11, each = 2 hours) for short advances, or `days` (0+) for multi-day travel. Total advancement = days * 12 + segments. Use 0 total to describe the current time without advancing. Use this when the player's action takes time. Describe why time passes in the reason field.",
    inputSchema,
    execute: wrapSafe(async (args: z.infer<typeof inputSchema>) => {
      const totalSegments = (args.days ?? 0) * 12 + (args.segments ?? 0);
      const { oldTime, newTime } = advanceGameTime(totalSegments);
      events.emitTimeUpdate(newTime.day, newTime.segment, totalSegments);
      const reasonStr = args.reason ? ` Reason: ${args.reason}.` : "";
      if (totalSegments === 0) {
        return `Time unchanged. It is still ${describeTime(newTime)}.`;
      }
      const parts: string[] = [];
      if (args.days && args.days > 0) parts.push(`${args.days} day(s)`);
      if (args.segments && args.segments > 0) parts.push(`${args.segments} segment(s)`);
      const label = parts.join(", ");
      return `Time advanced by ${label}.${reasonStr} It is now ${describeTime(newTime)} (was ${describeTime(oldTime)}).`;
    }, TOOL_NAMES.ADVANCE_TIME),
  });
}
