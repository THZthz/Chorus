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

export const SPEAKER_TYPES = [
  "YOU",
  "INNER_VOICE",
  "CHARACTER",
  "SYSTEM",
  "ROLL",
  "NOTIFICATION",
] as const;
export type SpeakerType = (typeof SPEAKER_TYPES)[number];

export const NOTIFICATION_TYPES = ["XP", "TASK", "ITEM"] as const;
export type NotificationType = (typeof NOTIFICATION_TYPES)[number];

export interface Message {
  id: string;
  speaker: string;
  type: SpeakerType;
  text: string;
  metadata?: {
    notificationType?: NotificationType;
  };
  skillCheck?: {
    skill: string;
    difficulty: string;
    success: boolean;
  };
  rollResult?: {
    dice: number[];
    total: number;
    difficulty: number;
    success: boolean;
    skill: string;
    skillBonus?: number;
  };
}

export interface DialogueOption {
  id: string;
  text: string;
  selectionMessage?: string; // First-person narration for the YOU message in dialogue history
  hintBefore?: string; // e.g. "[Consult the Void]"
  hintAfter?: string; // e.g. "[Charm her.]"
  nextStepId?: string; // Standard transition
  check?: {
    skill: string;
    difficulty: number;
    difficultyText: string;
    diceCount: number;
    isRed?: boolean; // High stakes, non-repeatable check
    conditions: {
      expression: string; // e.g. "success", "total < difficulty", "dice[0] === 1"
      stepId: string;
      label?: string; // Optional label for display
      color?: string; // Optional color for display
    }[];
  };
}

export interface DialogueStep {
  id: string;
  messages: (Omit<Message, "id"> & { id?: string })[];
  options: DialogueOption[];
}
