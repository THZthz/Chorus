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

import React, { useState, useRef } from "react";
import { GitBranch } from "lucide-react";
import { motion } from "motion/react";
import { DialogueOption } from "@/types/dialogue";

interface Props {
  options: DialogueOption[];
  onSelect: (option: DialogueOption) => void;
  unexploredOptionIds?: Set<string>;
  onCustomInput?: (text: string) => void;
  disabled?: boolean;
}

export const DialogueOptions: React.FC<Props> = ({
  options,
  onSelect,
  unexploredOptionIds,
  onCustomInput,
  disabled,
}) => {
  const [customText, setCustomText] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);

  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter" && customText.trim()) {
      e.preventDefault();
      onCustomInput?.(customText.trim());
      setCustomText("");
    } else if (e.key === "Escape") {
      setCustomText("");
      inputRef.current?.blur();
    }
  };

  return (
    <div className="mt-12 font-serif">
      <div className="h-px bg-gradient-to-r from-transparent via-white/10 to-transparent mb-8" />
      <div className="space-y-1">
        {options.map((option, index) => {
          const isRedCheck = option.check?.isRed;
          const isCustom = option.id.startsWith("custom_");
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
                    ? "text-accent/50 hover:text-[#ff8d61] border border-dashed border-white/10"
                    : "text-accent hover:text-[#ff8d61]"
              }`}
            >
              <div className="flex gap-2 items-start">
                <span className={`${isRedCheck ? "text-white" : "opacity-70"} whitespace-nowrap`}>
                  {index + 1}.
                </span>
                <span
                  className={`flex-1 ${!isRedCheck && "group-hover:underline underline-offset-4 decoration-1 decoration-accent/40 text-pretty"} ${isCustom ? "italic text-accent/50" : ""}`}
                >
                  {isCustom && (
                    <span className="not-italic text-[10px] uppercase tracking-wider text-white/20 mr-1.5">
                      custom
                    </span>
                  )}
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
      {onCustomInput && (
        <>
          <div className="h-px bg-gradient-to-r from-transparent via-white/5 to-transparent my-4" />
          <div className="px-1">
            <input
              ref={inputRef}
              type="text"
              value={customText}
              onChange={(e) => setCustomText(e.target.value)}
              onKeyDown={handleKeyDown}
              disabled={disabled}
              placeholder="Or type your own..."
              className="w-full bg-transparent border-b border-white/10 text-accent/60 placeholder:text-white/15 text-[16px] py-1.5 outline-none focus-visible:ring-1 focus-visible:ring-accent/50 focus:border-accent/40 focus:text-accent/90 transition-colors disabled:opacity-40"
            />
          </div>
        </>
      )}
    </div>
  );
};
