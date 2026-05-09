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

import React, { useState } from "react";
import { motion, AnimatePresence } from "motion/react";
import { Message, SpeakerType } from "@/types/dialogue";
import { DieFace } from "@/components/DiceRoller";
import { ObjectLink } from "@/components/ObjectLink";
import { VOICE_COLORS } from "@/shared/colors.ts";

interface Props {
  message: Message;
  isStreaming?: boolean;
  isFlashing?: boolean;
}

function hashNpcColor(name: string): string {
  let h = 5381;
  for (let i = 0; i < name.length; i++) {
    h = ((h << 5) + h) ^ name.charCodeAt(i);
  }
  const hue = Math.abs(h) % 360;
  return `hsl(${hue}, 48%, 66%)`;
}

function getSpeakerColor(speaker: string, type: SpeakerType): string {
  if (type === "INNER_VOICE") return VOICE_COLORS[speaker.toUpperCase()] ?? "#9081e3";
  if (type === "YOU") return "#d8d8d8";
  if (type === "SYSTEM") return "#6b7280";
  return hashNpcColor(speaker);
}

const RollTooltip: React.FC<{ rollResult: NonNullable<Message["rollResult"]> }> = ({
  rollResult,
}) => (
  <motion.div
    initial={{ opacity: 0, y: 5, scale: 0.95 }}
    animate={{ opacity: 1, y: 0, scale: 1 }}
    exit={{ opacity: 0, scale: 0.95 }}
    className="absolute bottom-full left-0 mb-4 p-4 bg-[#1a1510] brass-ring rounded-sm shadow-[0_10px_40px_rgba(0,0,0,0.8)] z-50 min-w-[180px]"
  >
    <div className="absolute -bottom-2 left-4 w-4 h-4 bg-[#1a1510] rotate-45 border-r border-b border-[#c4944a]/20" />
    <div className="text-[10px] text-[#c4944a]/60 uppercase tracking-[0.2em] mb-3 border-b border-[#c4944a]/10 pb-2"
      style={{ fontFamily: "'Barlow Condensed', 'Arial Narrow', sans-serif" }}>
      Roll Result
    </div>
    <div className="flex gap-2 mb-4 items-center">
      {rollResult.dice.map((val, i) => (
        <div
          key={i}
          className="w-10 h-10 rounded-sm border border-white/20 bg-[#222] flex items-center justify-center shadow-inner"
        >
          <DieFace value={val} size="md" />
        </div>
      ))}
      <div className="text-[14px] font-bold text-[#4fb0c6]">+{rollResult.skillBonus ?? 0}</div>
    </div>
    <div className="space-y-1">
      <div className="flex justify-between items-baseline">
        <span className="text-[10px] text-gray-500 uppercase tracking-wider">Total</span>
        <span className="text-[14px] font-bold text-white">{rollResult.total}</span>
      </div>
      <div className="flex justify-between items-baseline">
        <span className="text-[10px] text-gray-500 uppercase tracking-wider">Difficulty</span>
        <span className="text-[14px] font-bold text-white">{rollResult.difficulty}</span>
      </div>
      <div
        className={`pt-2 text-[12px] font-black uppercase tracking-[0.15em] ${rollResult.success ? "text-[#9eff9e]" : "text-[#ff6b6b]"}`}
      >
        {rollResult.success ? "Succeeded" : "Failed"}
      </div>
    </div>
  </motion.div>
);

export const DialogueMessage: React.FC<Props> = ({ message, isStreaming, isFlashing }) => {
  const [isTooltipVisible, setIsTooltipVisible] = useState(false);
  const isInnerVoice = message.type === "INNER_VOICE";
  const isSystem = message.type === "SYSTEM";
  const isNotification = message.type === "NOTIFICATION";
  const isYou = message.type === "YOU";

  const speakerColor = getSpeakerColor(message.speaker, message.type);

  const renderText = (text: string) => {
    const pattern = /(\*.*?\*|\[.*?\]\(#.*?\))/g;
    const parts = text.split(pattern);
    return parts.map((part, i) => {
      if (part.startsWith("*") && part.endsWith("*")) {
        return (
          <em key={i} className="italic opacity-90">
            {part.slice(1, -1)}
          </em>
        );
      }
      const objMatch = part.match(/\[(.*?)\]\(#(.*?)\)/);
      if (objMatch) {
        const [, displayName, objectId] = objMatch;
        return <ObjectLink key={i} displayName={displayName} objectId={objectId} />;
      }
      return part;
    });
  };

  // Compact status-line for notifications
  if (isNotification) {
    const hasRoll = !!message.rollResult;

    return (
      <motion.div
        initial={{ opacity: 0, y: 5 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.3 }}
        className="mb-6 relative"
      >
        <AnimatePresence>
          {isFlashing && (
            <motion.div
              key="flash"
              className="pointer-events-none absolute inset-0"
              initial={{ opacity: 0.55 }}
              animate={{ opacity: 0 }}
              transition={{ duration: 0.7, ease: "easeOut" }}
              style={{ background: "linear-gradient(90deg, #ff6b3522 0%, transparent 80%)" }}
            />
          )}
        </AnimatePresence>

        {hasRoll ? (
          <div className="brass-ring bg-[#0d0906]/60 p-5 rounded-sm">
            <div className="text-[15px] text-[#d8cfc0] italic leading-relaxed mb-4 pb-4 border-b border-[#c4944a]/10">
              &ldquo;{message.text}&rdquo;
            </div>
            <div className="flex gap-5 items-center">
              <div className="flex gap-2 items-center">
                {message.rollResult!.dice.map((val, i) => (
                  <div
                    key={i}
                    className="w-10 h-10 rounded-sm border border-[#c4944a]/30 bg-[#1a1510] flex items-center justify-center"
                  >
                    <DieFace value={val} size="md" />
                  </div>
                ))}
                <span className="text-[13px] text-[#7ec8e0] font-mono mx-0.5">+</span>
                <span className="text-[13px] text-[#7ec8e0] font-mono">
                  {message.rollResult!.skillBonus ?? 0}
                </span>
              </div>
              <div className="w-px h-10 bg-[#c4944a]/15" />
              <div className="space-y-0.5">
                <div className="flex items-baseline gap-2">
                  <span className="text-[10px] text-[#c4944a]/60 uppercase tracking-wider w-16"
                    style={{ fontFamily: "'Barlow Condensed', 'Arial Narrow', sans-serif" }}>
                    Skill
                  </span>
                  <span className="text-[13px] text-[#e8dcc8] font-mono">
                    {message.rollResult!.skill}
                  </span>
                </div>
                <div className="flex items-baseline gap-2">
                  <span className="text-[10px] text-[#c4944a]/60 uppercase tracking-wider w-16"
                    style={{ fontFamily: "'Barlow Condensed', 'Arial Narrow', sans-serif" }}>
                    Roll
                  </span>
                  <span className="text-[13px] text-[#e8dcc8] font-mono">
                    {message.rollResult!.total}
                    <span className="text-[#c4944a]/40"> vs </span>
                    {message.rollResult!.difficulty}
                  </span>
                </div>
              </div>
              <div className="ml-auto">
                <span
                  className="text-[12px] font-black uppercase tracking-[0.15em] px-3 py-1.5 border"
                  style={{
                    fontFamily: "'Barlow Condensed', 'Arial Narrow', sans-serif",
                    ...(message.rollResult!.success
                      ? { color: "#8fbc8f", borderColor: "rgba(143,188,143,0.3)", backgroundColor: "rgba(143,188,143,0.05)" }
                      : { color: "#d4786c", borderColor: "rgba(212,120,108,0.3)", backgroundColor: "rgba(212,120,108,0.05)" }
                    ),
                  }}
                >
                  {message.rollResult!.success ? "Success" : "Failure"}
                </span>
              </div>
            </div>
          </div>
        ) : (
          <div className="flex items-center gap-4">
            <div className="flex-1 h-px bg-[#c4944a]/8" />
            <div className="flex items-center gap-2">
              {message.skillCheck && (
                <div
                  className="relative inline-block"
                  onMouseEnter={() => setIsTooltipVisible(true)}
                  onMouseLeave={() => setIsTooltipVisible(false)}
                >
                  <span
                    className="text-[11px] cursor-help uppercase tracking-wider transition-opacity hover:opacity-100"
                    style={{
                      fontFamily: "'Barlow Condensed', 'Arial Narrow', sans-serif",
                      color: message.skillCheck.success ? "rgba(143,188,143,0.7)" : "rgba(212,120,108,0.7)",
                    }}
                  >
                    [{message.skillCheck.skill} · {message.skillCheck.success ? "Pass" : "Fail"}]
                  </span>
                  <AnimatePresence>
                    {isTooltipVisible && message.rollResult && (
                      <RollTooltip rollResult={message.rollResult} />
                    )}
                  </AnimatePresence>
                </div>
              )}
              <span className="text-[#5f8764]/60 text-[11px] uppercase tracking-[0.18em]">
                {message.text}
              </span>
            </div>
            <div className="flex-1 h-px bg-[#c4944a]/8" />
          </div>
        )}
      </motion.div>
    );
  }

  const paragraphs = (message.text ?? "").split(/(?:\r?\n){2,}/);

  let plateStyle: React.CSSProperties = {};
  if (isInnerVoice) {
    plateStyle = {
      background: `linear-gradient(90deg, ${speakerColor}10 0%, ${speakerColor}04 100%)`,
      borderLeft: `3px solid ${speakerColor}60`,
    };
  } else if (!isSystem && !isYou) {
    plateStyle = {
      background: "rgba(196,148,74,0.03)",
      borderTop: "1px solid rgba(196,148,74,0.12)",
      borderBottom: "1px solid rgba(196,148,74,0.12)",
    };
  } else if (isYou) {
    plateStyle = {
      background: "rgba(232,220,200,0.02)",
      borderLeft: "2px solid rgba(232,220,200,0.15)",
    };
  } else if (isSystem) {
    plateStyle = {
      background: "rgba(95,135,100,0.03)",
    };
  }

  return (
    <motion.div
      initial={{ opacity: 0, y: 12 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.4 }}
      className="mb-8 relative px-5 py-3 rounded-sm"
      style={plateStyle}
    >
      <AnimatePresence>
        {isFlashing && (
          <motion.div
            key="flash"
            className="pointer-events-none absolute inset-0 rounded-sm"
            initial={{ opacity: 0.4 }}
            animate={{ opacity: 0 }}
            transition={{ duration: 0.8, ease: "easeOut" }}
            style={{ background: "linear-gradient(90deg, #e8c54730 0%, transparent 70%)" }}
          />
        )}
      </AnimatePresence>
      {paragraphs.map((paragraphText, idx) => {
        if (!paragraphText.trim()) return null;

        return (
          <div key={idx} className={idx > 0 ? "mt-5" : ""}>
            {idx === 0 && (
              <div className="flex items-center gap-3 mb-2">
                {!isYou && !isSystem && (
                  <span
                    className="text-[13px] uppercase tracking-[0.2em] font-semibold leading-none"
                    style={{
                      fontFamily: "'Barlow Condensed', 'Arial Narrow', sans-serif",
                      color: speakerColor,
                      textShadow: "0 1px 0 rgba(0,0,0,0.4)",
                    }}
                  >
                    — {message.speaker} —
                  </span>
                )}
                {isYou && (
                  <span
                    className="text-[13px] uppercase tracking-[0.2em] font-semibold leading-none"
                    style={{
                      fontFamily: "'Barlow Condensed', 'Arial Narrow', sans-serif",
                      color: "#e8dcc8",
                      textShadow: "0 1px 0 rgba(0,0,0,0.4)",
                    }}
                  >
                    — YOU —
                  </span>
                )}

                {message.skillCheck && (
                  <div
                    className="relative inline-block"
                    onMouseEnter={() => setIsTooltipVisible(true)}
                    onMouseLeave={() => setIsTooltipVisible(false)}
                  >
                    <span className="text-white/25 text-[10px] uppercase cursor-help hover:text-white/55 transition-colors tracking-wider">
                      [{message.skillCheck.difficulty}:{" "}
                      <span
                        className={
                          message.skillCheck.success ? "text-[#8fbc8f]/70" : "text-[#d4786c]/70"
                        }
                      >
                        {message.skillCheck.success ? "Pass" : "Fail"}
                      </span>
                      ]
                    </span>
                    <AnimatePresence>
                      {isTooltipVisible && message.rollResult && (
                        <RollTooltip rollResult={message.rollResult} />
                      )}
                    </AnimatePresence>
                  </div>
                )}
              </div>
            )}

            <div
              className={`leading-[1.75] whitespace-pre-wrap text-pretty ${isSystem ? "text-[15px]" : "text-[17px]"}`}
              style={{
                fontFamily: "'Libre Baskerville', 'Georgia', serif",
                fontStyle: isInnerVoice ? "italic" : "normal",
                color: isSystem
                  ? "#5f8764"
                  : isInnerVoice
                    ? speakerColor
                    : isYou
                      ? "#e8dcc8"
                      : "#d8cfc0",
              }}
            >
              {renderText(paragraphText)}
              {isStreaming && idx === paragraphs.length - 1 && (
                <span className="inline-block w-1.5 h-1.5 bg-[#e8c547] rounded-full ml-0.5 align-middle animate-pulse"
                  style={{ boxShadow: "0 0 4px rgba(232,197,71,0.5)" }} />
              )}
            </div>
          </div>
        );
      })}
    </motion.div>
  );
};
