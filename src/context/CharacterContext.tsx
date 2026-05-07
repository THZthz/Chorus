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

interface CharacterContextType {
  character: Character;
  updateStat: (stat: keyof CharacterStats, value: number) => void;
  incrementStat: (stat: keyof CharacterStats) => void;
  getStatBySkillName: (skillName: string) => number;
}

const defaultCharacter: Character = {
  id: "player",
  type: "CHARACTER",
  displayName: "YOU",
  shortDescription: "An amnesiac waking in a strange bed at the Gilded Lotus.",
  longDescription: "You remember fragments — a letter sealed with black wax, a woman's scream, salt spray against stone. The rest is fog. You woke in a silk-draped room at the Gilded Lotus with Veyla's eyes on you and a dead woman's name in your pocket. Whatever brought you to Karavelle, you'll have to piece it together from the lies people tell.",
  attributes: {},
  opinions: {},
  stats: {
    logic: 3,
    rhetoric: 2,
    empathy: 3,
    perception: 4,
    volition: 2,
    endurance: 3,
    sorcery: 5,
    suggestion: 4,
    instinct: 1,
    might: 2,
    clockwork: 3,
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
