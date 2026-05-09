import React from "react";
import { motion } from "motion/react";

export const TypingIndicator: React.FC = () => {
  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      className="flex items-center gap-3 mb-8 pl-1"
    >
      {/* Amber pilot lamp */}
      <motion.div
        className="w-2 h-2 rounded-full"
        style={{
          background: "radial-gradient(circle, #e8c547, #c4944a)",
          boxShadow: "0 0 6px rgba(232,197,71,0.4), 0 0 12px rgba(232,197,71,0.15)",
        }}
        animate={{
          opacity: [1, 0.4, 1, 0.6, 1],
          boxShadow: [
            "0 0 6px rgba(232,197,71,0.4), 0 0 12px rgba(232,197,71,0.15)",
            "0 0 3px rgba(232,197,71,0.2), 0 0 6px rgba(232,197,71,0.08)",
            "0 0 6px rgba(232,197,71,0.4), 0 0 12px rgba(232,197,71,0.15)",
            "0 0 4px rgba(232,197,71,0.3), 0 0 8px rgba(232,197,71,0.1)",
            "0 0 6px rgba(232,197,71,0.4), 0 0 12px rgba(232,197,71,0.15)",
          ],
        }}
        transition={{
          duration: 1.5,
          repeat: Infinity,
          ease: "easeInOut",
          times: [0, 0.2, 0.3, 0.5, 1],
        }}
      />
      <span
        className="text-[10px] uppercase tracking-[0.2em] text-[#c4944a]/50"
        style={{ fontFamily: "'Barlow Condensed', 'Arial Narrow', sans-serif" }}
      >
        Thinking
      </span>
    </motion.div>
  );
};
