export type SpeakerType = 'YOU' | 'INNER_VOICE' | 'CHARACTER' | 'SYSTEM' | 'ROLL' | 'NOTIFICATION';

export interface Message {
  id: string;
  speaker: string;
  type: SpeakerType;
  text: string;
  metadata?: {
    notificationType?: 'XP' | 'TASK' | 'ITEM';
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
  hintBefore?: string; // e.g. "[Consult the Void]"
  hintAfter?: string; // e.g. "[Charm her.]"
  nextStepId?: string; // Standard transition
  isAiTrigger?: boolean; // If true, request response from LLM
  isContinue?: boolean; // If true, render as a large CONTINUE button
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
  messages: (Omit<Message, 'id'> & { id?: string })[];
  options: DialogueOption[];
}
