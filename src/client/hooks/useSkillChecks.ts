import { useState } from "react";
import { nextId } from "@/client/idPool";
import type { Message, DialogueOption } from "@/types/dialogue";

interface UseSkillChecksParams {
  getStatBySkillName: (skill: string) => number;
}

export function useSkillChecks({ getStatBySkillName }: UseSkillChecksParams) {
  const [isRolling, setIsRolling] = useState(false);

  const handleSkillCheck = async (
    option: DialogueOption,
    history: Message[],
    onComplete: (rollMessage: Message, updatedHistory: Message[]) => void,
  ): Promise<boolean> => {
    if (!option.check) return false;

    setIsRolling(true);
    const check = option.check;
    const diceCount = check.diceCount ?? 2;
    await new Promise((r) => setTimeout(r, 1000));
    const dice = Array.from({ length: diceCount }, () => Math.floor(Math.random() * 6) + 1);
    const skillBonus = getStatBySkillName(check.skill);
    const total = dice.reduce((a, b) => a + b, 0) + skillBonus;
    const success = total >= check.difficulty;

    const youText = history.length > 0 ? history[history.length - 1].text : "";

    const rollMessage: Message = {
      id: `roll-${await nextId()}`,
      speaker: "SYSTEM",
      type: "NOTIFICATION",
      text: youText,
      skillCheck: {
        skill: check.skill,
        difficulty: `${check.difficultyText} ${check.difficulty}`,
        success,
      },
      rollResult: {
        dice,
        total,
        difficulty: check.difficulty,
        success,
        skill: check.skill,
        skillBonus,
      },
    };

    const updatedHistory = [...history, rollMessage];
    setIsRolling(false);
    onComplete(rollMessage, updatedHistory);
    return true;
  };

  return { isRolling, handleSkillCheck };
}
