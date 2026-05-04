import React, { useState } from "react";
import { motion, AnimatePresence } from "motion/react";
import { Message, SpeakerType } from "@/types/dialogue";
import { DieFace } from "@/components/DiceRoller";
import { ObjectLink } from "@/components/ObjectLink";

interface Props {
  message: Message;
  isStreaming?: boolean;
  isFlashing?: boolean;
}

// Skill voice colors for the inner-mind pantheon
const VOICE_COLORS: Record<string, string> = {
  LOGIC: "#4fb0c6",
  RHETORIC: "#c6b050",
  EMPATHY: "#c67080",
  PERCEPTION: "#50c6a0",
  VOLITION: "#e07840",
  ENDURANCE: "#c05050",
  SORCERY: "#9081e3",
  SUGGESTION: "#a0c650",
  INSTINCT: "#e05858",
  MIGHT: "#50c060",
  CLOCKWORK: "#50b0c6",
  ALCHEMY: "#9eff9e",
};

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
    className="absolute bottom-full left-0 mb-4 p-4 bg-[#111] border border-white/10 rounded-sm shadow-[0_10px_40px_rgba(0,0,0,0.8)] z-50 min-w-[180px]"
  >
    <div className="absolute -bottom-2 left-4 w-4 h-4 bg-[#111] rotate-45 border-r border-b border-white/10" />
    <div className="text-[10px] uppercase tracking-[0.2em] text-gray-500 mb-3 border-b border-white/10 pb-2">
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
          <div className="border border-white/10 bg-[#0d0d0d] rounded-sm p-5">
            {/* Option text */}
            <div className="text-[15px] text-[#ccc] italic leading-relaxed mb-4 pb-4 border-b border-white/5">
              &ldquo;{message.text}&rdquo;
            </div>

            {/* Dice + stats row */}
            <div className="flex gap-5 items-center">
              {/* Dice faces */}
              <div className="flex gap-2 items-center">
                {message.rollResult!.dice.map((val, i) => (
                  <div
                    key={i}
                    className="w-10 h-10 rounded-sm border border-white/20 bg-[#1a1a1a] flex items-center justify-center"
                  >
                    <DieFace value={val} size="md" />
                  </div>
                ))}
                <span className="text-[13px] text-[#4fb0c6] font-mono mx-0.5">+</span>
                <span className="text-[13px] text-[#4fb0c6] font-mono">
                  {message.rollResult!.skillBonus ?? 0}
                </span>
              </div>

              {/* Vertical divider */}
              <div className="w-px h-10 bg-white/10" />

              {/* Stat lines */}
              <div className="space-y-0.5">
                <div className="flex items-baseline gap-2">
                  <span className="text-[10px] text-gray-500 uppercase tracking-wider w-16">
                    Skill
                  </span>
                  <span className="text-[13px] text-white font-mono">
                    {message.rollResult!.skill}
                  </span>
                </div>
                <div className="flex items-baseline gap-2">
                  <span className="text-[10px] text-gray-500 uppercase tracking-wider w-16">
                    Roll
                  </span>
                  <span className="text-[13px] text-white font-mono">
                    {message.rollResult!.total}
                    <span className="text-gray-500"> vs </span>
                    {message.rollResult!.difficulty}
                  </span>
                </div>
              </div>

              {/* Result badge */}
              <div className="ml-auto">
                <span
                  className={`text-[12px] font-black uppercase tracking-[0.15em] px-3 py-1.5 border ${
                    message.rollResult!.success
                      ? "text-[#9eff9e] border-[#9eff9e]/30 bg-[#9eff9e]/5"
                      : "text-[#ff6b6b] border-[#ff6b6b]/30 bg-[#ff6b6b]/5"
                  }`}
                >
                  {message.rollResult!.success ? "Success" : "Failure"}
                </span>
              </div>
            </div>
          </div>
        ) : (
          <div className="flex items-center gap-4">
            <div className="flex-1 h-px bg-[#a3c2a3]/12" />
            <div className="flex items-center gap-2">
              {message.skillCheck && (
                <div
                  className="relative inline-block"
                  onMouseEnter={() => setIsTooltipVisible(true)}
                  onMouseLeave={() => setIsTooltipVisible(false)}
                >
                  <span
                    className={`text-[11px] font-mono cursor-help uppercase tracking-wider transition-opacity hover:opacity-100 ${message.skillCheck.success ? "text-[#9eff9e]/70" : "text-[#ff6b6b]/70"}`}
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
              <span className="text-[#a3c2a3]/60 text-[11px] uppercase tracking-[0.18em] font-mono">
                {message.text}
              </span>
            </div>
            <div className="flex-1 h-px bg-[#a3c2a3]/12" />
          </div>
        )}
      </motion.div>
    );
  }

  const paragraphs = (message.text ?? "").split(/(?:\r?\n){2,}/);

  // Inner voice: colored left border matches the voice's identity color
  const borderStyle = isInnerVoice ? { borderLeftColor: speakerColor + "55" } : undefined;

  return (
    <motion.div
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.4 }}
      className={`mb-8 relative ${isInnerVoice ? "border-l-2 pl-4 font-serif" : isYou ? "font-['Times_New_Roman',Times]" : "font-serif"}`}
      style={borderStyle}
    >
      <AnimatePresence>
        {isFlashing && (
          <motion.div
            key="flash"
            className="pointer-events-none absolute inset-0"
            initial={{ opacity: 0.6 }}
            animate={{ opacity: 0 }}
            transition={{ duration: 0.8, ease: "easeOut" }}
            style={{ background: "linear-gradient(90deg, #ff6b3530 0%, transparent 70%)" }}
          />
        )}
      </AnimatePresence>
      {paragraphs.map((paragraphText, idx) => {
        if (!paragraphText.trim()) return null;

        return (
          <div key={idx} className={idx > 0 ? "mt-5" : ""}>
            {/* Speaker label — shown only on the first paragraph */}
            {idx === 0 && (
              <div className="flex items-center gap-3 mb-2">
                <span
                  className="font-mono text-[22px] uppercase tracking-[0.001em] font-semibold leading-none"
                  style={{ color: speakerColor }}
                >
                  {message.speaker}
                </span>

                {message.skillCheck && (
                  <div
                    className="relative inline-block"
                    onMouseEnter={() => setIsTooltipVisible(true)}
                    onMouseLeave={() => setIsTooltipVisible(false)}
                  >
                    <span className="text-white/35 text-[10px] font-mono uppercase cursor-help hover:text-white/65 transition-colors tracking-wider">
                      [{message.skillCheck.difficulty}:{" "}
                      <span
                        className={
                          message.skillCheck.success ? "text-[#9eff9e]/70" : "text-[#ff6b6b]/70"
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

            {/* Dialogue body */}
            <div
              className={`leading-[1.75] whitespace-pre-wrap ${isSystem ? "text-[16px] text-gray-500" : "text-[18px]"}`}
              style={isInnerVoice ? { color: speakerColor } : { color: "#e8e8e8" }}
            >
              {renderText(paragraphText)}
              {isStreaming && idx === paragraphs.length - 1 && (
                <span className="inline-block w-[2px] h-[1em] bg-[#ff6b35] ml-0.5 align-text-bottom animate-pulse" />
              )}
            </div>
          </div>
        );
      })}
    </motion.div>
  );
};
