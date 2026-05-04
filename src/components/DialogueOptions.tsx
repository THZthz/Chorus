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

import React from "react";
import { GitBranch } from "lucide-react";
import { motion } from "motion/react";
import { DialogueOption } from "@/types/dialogue";

interface Props {
  options: DialogueOption[];
  onSelect: (option: DialogueOption) => void;
  unexploredOptionIds?: Set<string>;
}

export const DialogueOptions: React.FC<Props> = ({ options, onSelect, unexploredOptionIds }) => {
  return (
    <div className="mt-12 font-serif">
      <div className="h-px bg-gradient-to-r from-transparent via-white/10 to-transparent mb-8" />
      <div className="space-y-1">
        {options.map((option, index) => {
          const isRedCheck = option.check?.isRed;
          const skillCheckHint = option.check
            ? `[${option.check.skill} - ${option.check.difficultyText || "Unknown"} ${option.check.difficulty}]`
            : null;

          const isUnexplored = unexploredOptionIds?.has(option.id) ?? false;

          return (
            <motion.button
              key={option.id}
              initial={{ opacity: 0, x: -10 }}
              animate={{ opacity: 1, x: 0 }}
              transition={{ delay: 0.1 * index }}
              onClick={() => onSelect(option)}
              title={isUnexplored ? "Unexplored branch — will generate new content" : undefined}
              className={`group block w-full text-left text-[18px] transition-colors p-2 -ml-2 rounded-sm ${
                isRedCheck
                  ? "bg-[#d34b34] text-white hover:bg-[#e05a44]"
                  : isUnexplored
                    ? "text-[#ff6b35]/50 hover:text-[#ff8d61] border border-dashed border-white/10"
                    : "text-[#ff6b35] hover:text-[#ff8d61]"
              }`}
            >
              <div className="flex gap-2 items-start">
                <span className={`${isRedCheck ? "text-white" : "opacity-70"} whitespace-nowrap`}>
                  {index + 1}.
                </span>
                <span
                  className={`flex-1 ${!isRedCheck && "group-hover:underline underline-offset-4 decoration-1 decoration-[#ff6b35]/40 text-pretty"}`}
                >
                  {skillCheckHint && (
                    <span
                      className={`font-bold mr-2 ${isRedCheck ? "text-white" : "text-[#4fb0c6]"}`}
                    >
                      {skillCheckHint}
                    </span>
                  )}
                  {option.hintBefore && !option.check && (
                    <span className="font-bold mr-1">{option.hintBefore}</span>
                  )}
                  {option.text}
                  {option.hintAfter && <span className="font-bold ml-1">{option.hintAfter}</span>}
                </span>
                {isUnexplored && (
                  <span className="flex-shrink-0 text-emerald-400/40 mt-1">
                    <GitBranch size={10} />
                  </span>
                )}
              </div>
            </motion.button>
          );
        })}
      </div>
    </div>
  );
};
