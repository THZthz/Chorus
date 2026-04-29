import React, { useState } from 'react';
import { motion, AnimatePresence } from 'motion/react';
import { Message } from '@/types/dialogue';
import { DieFace } from '@/components/DiceRoller';
import { ObjectLink } from '@/components/ObjectLink';

interface Props {
  message: Message;
}

export const DialogueMessage: React.FC<Props> = ({ message }) => {
  const [isTooltipVisible, setIsTooltipVisible] = useState(false);
  const isInnerVoice = message.type === 'INNER_VOICE';
  const isYou = message.type === 'YOU';
  const isSystem = message.type === 'SYSTEM';
  const isNotification = message.type === 'NOTIFICATION';

  const getSpeakerColor = () => {
    if (isInnerVoice) return 'text-[#9081e3]'; // Light Purple
    if (isSystem) return 'text-gray-400';
    if (isNotification) return 'text-[#a3c2a3]'; // Muted Green
    return 'text-white font-medium';
  };

  const renderText = (text: string) => {
    // 1. Split for italics: *text*
    // 2. Split for object links: [display](#id)
    // We combine these into a robust parsing strategy

    // First, identify all special patterns and split the text
    // Regex matches either *italic* or [object](#id)
    const pattern = /(\*.*?\*|\[.*?\]\(#.*?\))/g;
    const parts = text.split(pattern);

    return parts.map((part, i) => {
      // Handle Italics
      if (part.startsWith('*') && part.endsWith('*')) {
        return <em key={i} className="italic opacity-90">{part.slice(1, -1)}</em>;
      }

      // Handle Object Links: [name](#id)
      const objMatch = part.match(/\[(.*?)\]\(#(.*?)\)/);
      if (objMatch) {
        const [, displayName, objectId] = objMatch;
        return <ObjectLink key={i} displayName={displayName} objectId={objectId} />;
      }

      return part;
    });
  };

  return (
    <motion.div
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.4 }}
      className="mb-8 font-serif leading-relaxed text-[18px]"
    >
      <div className="flex flex-wrap items-baseline gap-2">
        {!isNotification && (
          <span className={`${getSpeakerColor()} uppercase tracking-wider text-[16px] mr-2`}>
            {message.speaker}
          </span>
        )}
        {message.skillCheck && (
          <div
            className="relative inline-block"
            onMouseEnter={() => setIsTooltipVisible(true)}
            onMouseLeave={() => setIsTooltipVisible(false)}
          >
            <span className="text-white/40 text-[14px] mr-2 uppercase cursor-help hover:text-white/80 transition-colors">
              [{message.skillCheck.difficulty}: <span className={message.skillCheck.success ? 'text-[#9eff9e]' : 'text-[#ff6b6b]'}>{message.skillCheck.success ? 'Success' : 'Failure'}</span>]
            </span>

            <AnimatePresence>
              {isTooltipVisible && message.rollResult && (
                <motion.div
                  initial={{ opacity: 0, y: 5, scale: 0.95 }}
                  animate={{ opacity: 1, y: 0, scale: 1 }}
                  exit={{ opacity: 0, scale: 0.95 }}
                  className="absolute bottom-full left-0 mb-4 p-4 bg-[#111] border border-white/10 rounded-sm shadow-[0_10px_40px_rgba(0,0,0,0.8)] z-50 min-w-[180px]"
                >
                  <div className="absolute -bottom-2 left-4 w-4 h-4 bg-[#111] rotate-45 border-r border-b border-white/10" />

                  <div className="text-[10px] uppercase tracking-[0.2em] text-gray-500 mb-3 border-b border-white/10 pb-2">Roll Result</div>

                  <div className="flex gap-2 mb-4 items-center">
                    {message.rollResult.dice.map((val, idx) => (
                      <div key={idx} className="w-10 h-10 rounded-sm border border-white/20 bg-[#222] flex items-center justify-center shadow-inner">
                        <DieFace value={val} size="md" />
                      </div>
                    ))}
                    <div className="text-[14px] font-bold text-[#4fb0c6]">
                      +{message.rollResult.skillBonus ?? 0}
                    </div>
                  </div>

                  <div className="space-y-1">
                    <div className="flex justify-between items-baseline">
                      <span className="text-[10px] text-gray-500 uppercase tracking-wider">Total</span>
                      <span className="text-[14px] font-bold text-white">{message.rollResult.total}</span>
                    </div>
                    <div className="flex justify-between items-baseline">
                      <span className="text-[10px] text-gray-500 uppercase tracking-wider">Difficulty</span>
                      <span className="text-[14px] font-bold text-white">{message.rollResult.difficulty}</span>
                    </div>
                    <div className={`pt-2 text-[12px] font-black uppercase tracking-[0.15em] ${message.rollResult.success ? 'text-[#9eff9e]' : 'text-[#ff6b6b]'}`}>
                      {message.rollResult.success ? 'Succeeded' : 'Failed'}
                    </div>
                  </div>
                </motion.div>
              )}
            </AnimatePresence>
          </div>
        )}
      </div>
      <div className={`${isInnerVoice ? 'text-[#9081e3]' : isNotification ? 'text-[#a3c2a3]' : 'text-gray-100'} mt-1`}>
        {renderText(message.text)}
      </div>
    </motion.div>
  );
};
