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

import type { Message, DialogueOption } from "@/types/dialogue";

export function buildHistoryFromTree(
  stepId: string,
  treeSteps: Record<
    string,
    {
      id: string;
      parentStepId: string | null;
      parentOptionId: string | null;
      messages: Message[];
      options: DialogueOption[];
    }
  >,
): Message[] {
  const chain: (typeof treeSteps)[string][] = [];
  let cur: (typeof treeSteps)[string] | undefined = treeSteps[stepId];
  while (cur) {
    chain.unshift(cur);
    cur = cur.parentStepId ? treeSteps[cur.parentStepId] : undefined;
  }
  const result: Message[] = [];
  for (let i = 0; i < chain.length; i++) {
    const step = chain[i];
    if (i > 0) {
      const parent = chain[i - 1];
      const opt = parent.options.find((o) => o.id === step.parentOptionId);
      if (opt) {
        const youText = opt.selectionMessage ?? opt.text.replace(/^\[[^\]]*?:[^\]]*?\]\s*/, "");
        result.push({ id: `you-tree-${i}`, speaker: "YOU", type: "YOU", text: youText });
      } else if (!step.parentOptionId) {
        result.push({ id: `you-tree-${i}`, speaker: "YOU", type: "YOU", text: "[Free choice]" });
      }
    }
    result.push(...step.messages);
  }
  return result;
}
