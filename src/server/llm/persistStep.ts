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

import { nextId } from "@/server/models/ids";
import {
  saveStep,
  deactivateSiblingBranches,
  updateOptionNextStepId,
  addOptionToStep,
} from "@/server/models/dialogue";
import { addMessage } from "@/server/models/history";
import { getAllEntities } from "@/server/models/world";
import { getAllPlots } from "@/server/models/plot";
import { getGameTime, getSceneState } from "@/server/models/scene";
import { getFactsSnapshot } from "@/server/models/facts";
import type { Message, DialogueOption } from "@/types/dialogue";
import type { Character } from "@/types/entities";

export function persistStep(
  stepId: string,
  parentStepId: string | null,
  parentOptionId: string | null,
  messages: Message[],
  options: DialogueOption[],
  playerCharacter: Character | null,
  label: string,
  userInput: string | null,
) {
  // Custom input: parentStepId set but no parentOptionId → create a synthetic option
  // on the parent step so this branch is navigable in replay mode.
  let effectiveParentOptionId = parentOptionId;
  if (parentStepId && !effectiveParentOptionId) {
    const customOptionId = `custom_${nextId()}`;
    const optionText = (userInput ?? "Custom input").slice(0, 120);
    const customOption: DialogueOption = {
      id: customOptionId,
      text: optionText.length >= 120 ? optionText.slice(0, 117) + "…" : optionText,
      selectionMessage: userInput ?? "Custom input",
    };
    addOptionToStep(parentStepId, customOption);
    effectiveParentOptionId = customOptionId;
    console.log(`[${label}] synthetic custom option: ${parentStepId}.${customOptionId}`);
  }

  saveStep({
    id: stepId,
    parentStepId,
    parentOptionId: effectiveParentOptionId,
    messages,
    options,
    worldSnapshot: {
      entities: getAllEntities(),
      plots: getAllPlots(),
      playerCharacter,
      gameTime: getGameTime(),
      scene: getSceneState(),
      facts: getFactsSnapshot(),
    },
    isGenerated: true,
    isActive: true,
  });

  console.log(
    `[${label}] persisted step=${stepId} messages=${messages.length} options=${options.length}`,
  );

  if (parentStepId && effectiveParentOptionId) {
    updateOptionNextStepId(parentStepId, effectiveParentOptionId, stepId);
    console.log(
      `[${label}] linked parent option: ${parentStepId}.${effectiveParentOptionId} -> ${stepId}`,
    );
  }

  for (const msg of messages) {
    try {
      addMessage(msg);
    } catch (err: unknown) {
      const message = err instanceof Error ? err.message : String(err);
      if (!message.includes("UNIQUE constraint failed")) {
        console.error("Failed to save message to history:", message);
      }
    }
  }

  if (parentStepId) {
    deactivateSiblingBranches(parentStepId, stepId);
  }
}
