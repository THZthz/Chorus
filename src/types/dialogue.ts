/**
 * Chorus — cinematic dialogue engine
 * Copyright (C) 2026 Amias 1289941679@qq.com
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

import { SkillName } from "@/shared/constants";

export const NOTIFICATION_TYPES = ["TASK", "INFO", "WARNING", "ITEM_RECEIVED"] as const;
export type NotificationType = (typeof NOTIFICATION_TYPES)[number];

export const SPEAKER_TYPES = [
  "INNER_VOICE",
  "CHARACTER",
  "SYSTEM",
  "ROLL",
  "NOTIFICATION",
  "YOU",
] as const;
export type SpeakerType = (typeof SPEAKER_TYPES)[number];

export interface Message {
  id: string;
  speaker: string;
  type: SpeakerType;
  text: string;
  metadata?: {
    notificationType?: NotificationType;
  };
  rollResult?: {
    skill: SkillName;
    difficulty: number;
    dice: number[];
    total: number;
    success: boolean;
  };
}

export interface DialogueOption {
  text: string;
  hintBefore?: string;
  hintAfter?: string;
  check?: {
    skill: SkillName;
    difficulty: number;
    difficultyText: string;
    diceCount: number;
    conditions?: Array<{
      expression: string;
      label?: string;
      color?: string;
      stepId?: string;
    }>;
  };
}
