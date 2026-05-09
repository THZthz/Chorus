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

import { ReactNode, createContext, useContext, useState } from "react";
import { Character, CharacterStats } from "@/types/entities";
import { PLAYER_ID } from "@/shared/constants";

interface CharacterContextType {
  character: Character;
  updateStat: (stat: keyof CharacterStats, value: number) => void;
  incrementStat: (stat: keyof CharacterStats) => void;
  getStatBySkillName: (skillName: string) => number;
}

const defaultCharacter: Character = {
  id: PLAYER_ID,
  type: "CHARACTER",
  displayName: "YOU",
  shortDescription:
    "An amnesiac with a glowing crystal and a secret power, waking in the Velvet Thorn brothel deep in the Warrens.",
  longDescription:
    "You remember nothing before the rain. Cold cobblestones. The distant clang of a harbor bell. A woman's voice, low and urgent, calling you back from somewhere dark. Then the warm glow of a violet crystal in your palm, pulsing in time with your heartbeat. You woke in a velvet-draped room at the Velvet Thorn, a brothel in the Warrens, with Veyla's golden eyes watching you and a name you don't recognize on her lips. The shard responds to your emotions — flaring bright when your pulse quickens, when Veyla draws close, when desire or fear or fury surge through you. You don't know what you are. You don't know where the power comes from. But in a city where unlicensed magic is a crime and the Warrens devour the weak, ignorance is a death sentence — and what burns between you and the half-elf who saved you might be the only truth worth trusting.",
  attributes: {},
  opinions: {},
  conditions: {},
  stats: {
    logic: 3,
    rhetoric: 2,
    empathy: 4,
    perception: 4,
    volition: 3,
    endurance: 3,
    sorcery: 6,
    suggestion: 5,
    instinct: 3,
    might: 2,
    clockwork: 2,
    alchemy: 2,
  },
};

const CharacterContext = createContext<CharacterContextType | undefined>(undefined);

export function CharacterProvider({ children }: { children: ReactNode }) {
  const [character, setCharacter] = useState<Character>(defaultCharacter);

  const updateStat = (stat: keyof CharacterStats, value: number) => {
    setCharacter((prev) => ({
      ...prev,
      stats: {
        ...prev.stats,
        [stat]: value,
      },
    }));
  };

  const incrementStat = (stat: keyof CharacterStats) => {
    setCharacter((prev) => ({
      ...prev,
      stats: {
        ...prev.stats,
        [stat]: prev.stats[stat] + 1,
      },
    }));
  };

  const getStatBySkillName = (skillName: string): number => {
    const formatKey = skillName.toLowerCase().replace(/\s+/g, "_") as keyof CharacterStats;
    return character.stats[formatKey] || 0;
  };

  return (
    <CharacterContext.Provider value={{ character, updateStat, incrementStat, getStatBySkillName }}>
      {children}
    </CharacterContext.Provider>
  );
}

export function useCharacter() {
  const context = useContext(CharacterContext);
  if (context === undefined) {
    throw new Error("useCharacter must be used within a CharacterProvider");
  }
  return context;
}
