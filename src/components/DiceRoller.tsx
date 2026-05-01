import React, { useState, useEffect } from "react";
import { motion, AnimatePresence } from "motion/react";
import { useCharacter } from "@/context/CharacterContext";

interface Props {
  skill: string;
  difficulty: number;
  difficultyText: string;
  diceCount: number;
  isRed?: boolean;
  conditions?: {
    expression: string;
    stepId: string;
    label?: string;
    color?: string;
  }[];
  onComplete: (total: number, success: boolean, dice: number[]) => void;
}

// Helper to calculate 2D6 probabilities
const calculate2D6Probability = (target: number, bonus: number) => {
  const neededOnDice = target - bonus;
  if (neededOnDice <= 2) return 100;
  if (neededOnDice > 12) return 0;

  // Total outcomes: 36
  let successes = 0;
  for (let d1 = 1; d1 <= 6; d1++) {
    for (let d2 = 1; d2 <= 6; d2++) {
      if (d1 + d2 >= neededOnDice) successes++;
    }
  }
  return Math.round((successes / 36) * 100);
};

export const DieFace: React.FC<{ value: number; size?: "sm" | "md" | "lg" | "xs" }> = ({
  value,
  size = "md",
}) => {
  // Map values to 3x3 grid positions (0-8)
  const dotPositions: Record<number, number[]> = {
    1: [4],
    2: [2, 6],
    3: [2, 4, 6],
    4: [0, 2, 6, 8],
    5: [0, 2, 4, 6, 8],
    6: [0, 3, 6, 2, 5, 8],
  };

  const dots = dotPositions[value] || [];
  const sizes = {
    xs: { container: "w-3 h-3", dot: "w-0.5 h-0.5", gap: "gap-0" },
    sm: { container: "w-5 h-5", dot: "w-1 h-1", gap: "gap-0.5" },
    md: { container: "w-8 h-8", dot: "w-1.5 h-1.5", gap: "gap-1" },
    lg: { container: "w-12 h-12", dot: "w-2 h-2", gap: "gap-1.5" },
  };

  const currentSize = sizes[size];

  return (
    <div
      className={`grid grid-cols-3 grid-rows-3 ${currentSize.gap} ${currentSize.container} pointer-events-none`}
    >
      {[...Array(9)].map((_, i) => (
        <div key={i} className="flex items-center justify-center">
          {dots.includes(i) && (
            <div
              className={`${currentSize.dot} rounded-full bg-white shadow-[0_0_2px_rgba(255,255,255,0.5)]`}
            />
          )}
        </div>
      ))}
    </div>
  );
};

// Unified Box Container
const RollerBox = ({
  children,
  onClick,
  isRolling,
  hasRolled,
  outcome,
  skill,
}: {
  children: React.ReactNode;
  onClick?: () => void;
  isRolling: boolean;
  hasRolled: boolean;
  outcome: { label: string; color: string; isSuccess: boolean };
  skill: string;
}) => (
  <motion.div
    initial={{ opacity: 0 }}
    animate={{ opacity: 1 }}
    exit={{ opacity: 0 }}
    className="fixed inset-0 z-[100] flex items-center justify-center p-4 bg-black/40 backdrop-blur-sm"
  >
    <motion.div
      initial={{ opacity: 0, scale: 0.95, y: 20 }}
      animate={{ opacity: 1, scale: 1, y: 0 }}
      className="relative"
    >
      <div
        className={`w-80 bg-[#050505] border border-white/10 shadow-[0_0_100px_rgba(0,0,0,1)] overflow-hidden rounded-sm relative transition-all duration-500 ${onClick ? "cursor-pointer group hover:border-white/20" : ""}`}
        onClick={onClick}
      >
        {/* Subtle grid pattern */}
        <div className="absolute inset-0 opacity-[0.03] pointer-events-none bg-[radial-gradient(#fff_1px,transparent_1px)] [background-size:16px_16px]" />

        {/* Header bar */}
        <div
          className={`px-4 py-2 text-white font-sans font-bold uppercase tracking-[0.2em] text-center text-[11px] transition-colors duration-500 ${isRolling ? "bg-[#4fb0c6]" : hasRolled ? (outcome.isSuccess ? "bg-[#2d5a27]" : "bg-[#5a2727]") : "bg-[#1a1a1a]"}`}
        >
          {skill} Check {hasRolled ? (outcome.isSuccess ? "Passed" : "Failed") : ""}
        </div>

        <div className="p-8 flex flex-col items-center text-center relative z-10 min-h-[340px] justify-center">
          {children}
        </div>

        {/* Prompt overlay */}
        {!isRolling && !hasRolled && onClick && (
          <div className="absolute inset-x-0 bottom-4 opacity-0 group-hover:opacity-100 transition-opacity flex justify-center pointer-events-none">
            <div className="text-white text-[9px] font-bold tracking-[0.3em] uppercase opacity-40">
              Click to Proceed
            </div>
          </div>
        )}
      </div>
    </motion.div>
  </motion.div>
);

export const DiceRoller: React.FC<Props> = ({
  skill,
  difficulty,
  difficultyText,
  diceCount,
  isRed,
  conditions,
  onComplete,
}) => {
  const { getStatBySkillName } = useCharacter();
  const skillBonus = getStatBySkillName(skill);
  const [dice, setDice] = useState<number[]>(new Array(diceCount).fill(1));
  const [isRolling, setIsRolling] = useState(false);
  const [hasRolled, setHasRolled] = useState(false);
  const [rollCount, setRollCount] = useState(0);

  const probability = calculate2D6Probability(difficulty, skillBonus);

  useEffect(() => {
    let interval: NodeJS.Timeout;
    if (isRolling) {
      interval = setInterval(() => {
        setDice((prev) => prev.map(() => Math.floor(Math.random() * 6) + 1));
        setRollCount((c) => c + 1);

        // Stop rolling after ~2.8 seconds (7 * 400ms)
        // This makes the rolls feel discrete and rhythmic
        if (rollCount > 6) {
          setIsRolling(false);
          setHasRolled(true);
          const finalDice = new Array(diceCount)
            .fill(0)
            .map(() => Math.floor(Math.random() * 6) + 1);
          setDice(finalDice);
          const diceTotal = finalDice.reduce((a, b) => a + b, 0);
          const totalWithBonus = diceTotal + skillBonus;

          // Delay before completing to show final result
          setTimeout(() => {
            onComplete(totalWithBonus, totalWithBonus >= difficulty, finalDice);
          }, 1500);
        }
      }, 400);
    }
    return () => clearInterval(interval);
  }, [isRolling, rollCount, skillBonus, difficulty, diceCount, onComplete]);

  const getOutcome = () => {
    const diceTotal = dice.reduce((a, b) => a + b, 0);
    const total = diceTotal + skillBonus;
    const success = total >= difficulty;

    if (conditions && hasRolled) {
      for (const cond of conditions) {
        try {
          const evaluator = new Function(
            "dice",
            "total",
            "success",
            "diceLen",
            `return ${cond.expression}`,
          );
          if (evaluator(dice, total, success, dice.length)) {
            return {
              label: cond.label || "Special Outcome",
              color: cond.color || "text-purple-400",
              isSuccess: success,
            };
          }
        } catch (e) {
          console.error("Error evaluating outcome:", e);
        }
      }
    }

    return success
      ? { label: "Succeeded", color: "text-[#9eff9e]", isSuccess: true }
      : { label: "Failed", color: "text-[#ff6b6b]", isSuccess: false };
  };

  const outcome = getOutcome();
  const diceTotal = dice.reduce((a, b) => a + b, 0);
  const currentTotal = diceTotal + skillBonus;

  const rollerProps = { isRolling, hasRolled, outcome, skill };

  // 1. Red Check Idle
  if (isRed && !isRolling && !hasRolled) {
    return (
      <RollerBox {...rollerProps} onClick={() => setIsRolling(true)}>
        <div className="text-[#ff4d4d] text-[18px] uppercase tracking-[0.4em] font-black mb-2 drop-shadow-[0_0_8px_rgba(255,77,77,0.3)]">
          RED CHECK
        </div>
        <div className="text-[64px] font-bold text-white leading-none mb-4 tracking-tighter">
          {probability}%
        </div>

        <div className="w-16 h-[1px] bg-white/10 mb-6" />

        <div className="text-gray-400 text-[12px] font-serif italic mb-8 max-w-[220px] leading-relaxed">
          This action is irreversible. Failure will be absolute.
        </div>

        <div className="flex justify-between w-full mt-4 opacity-30 px-6">
          <div className="flex flex-col items-center gap-2">
            <div className="flex gap-1">
              <div className="w-4 h-4 bg-white/10 rounded-sm flex items-center justify-center">
                <DieFace value={1} size="xs" />
              </div>
              <div className="w-4 h-4 bg-white/10 rounded-sm flex items-center justify-center">
                <DieFace value={1} size="xs" />
              </div>
            </div>
            <span className="text-[6px] uppercase tracking-widest">Natural 2</span>
          </div>

          <div className="flex flex-col items-center gap-2">
            <div className="flex gap-1">
              <div className="w-4 h-4 bg-white/10 rounded-sm flex items-center justify-center">
                <DieFace value={6} size="xs" />
              </div>
              <div className="w-4 h-4 bg-white/10 rounded-sm flex items-center justify-center">
                <DieFace value={6} size="xs" />
              </div>
            </div>
            <span className="text-[6px] uppercase tracking-widest">Natural 12</span>
          </div>
        </div>
      </RollerBox>
    );
  }

  // 2. Standard Check Idle
  if (!isRolling && !hasRolled) {
    return (
      <RollerBox {...rollerProps} onClick={() => setIsRolling(true)}>
        <div className="text-[#4fb0c6] text-[13px] uppercase tracking-[0.4em] font-bold mb-4 opacity-70">
          Probability
        </div>
        <div className="text-[72px] font-bold text-white leading-none mb-2 tracking-tighter">
          {probability}%
        </div>

        <div className="w-full max-w-[160px] h-1 bg-white/5 rounded-full overflow-hidden mb-8 mt-2">
          <motion.div
            initial={{ width: 0 }}
            animate={{ width: `${probability}%` }}
            className="h-full bg-[#4fb0c6]"
          />
        </div>

        <div className="flex gap-4 items-center text-[12px] font-serif italic text-gray-400">
          <span>Stat: +{skillBonus}</span>
          <span className="w-1 h-1 rounded-full bg-white/20" />
          <span>Diff: {difficulty}</span>
        </div>
      </RollerBox>
    );
  }

  // 3. Rolling
  if (isRolling) {
    return (
      <RollerBox {...rollerProps}>
        <div className="absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 text-[100px] font-black text-white/[0.02] select-none pointer-events-none tracking-tighter">
          ACTIVE
        </div>

        <div className="flex gap-6 mb-8 relative z-20">
          {dice.map((value, i) => (
            <motion.div
              key={i}
              animate={{
                rotate: [0, 90, 180, 270, 360],
                scale: [1, 1.1, 1],
                y: [0, -10, 0],
              }}
              transition={{ duration: 0.4, repeat: Infinity, ease: "linear" }}
              className="w-16 h-16 bg-[#111] border border-white/20 rounded shadow-[0_0_30px_rgba(255,255,255,0.05)] flex items-center justify-center"
            >
              <DieFace value={value} size="md" />
            </motion.div>
          ))}
        </div>

        <div className="mt-4 flex flex-col items-center">
          <div className="text-[10px] font-black text-[#4fb0c6] tracking-[0.4em] uppercase mb-2">
            Calculating Outcome
          </div>
          <div className="flex gap-1.5 h-1">
            {[...Array(5)].map((_, i) => (
              <motion.div
                key={i}
                animate={{
                  opacity: [0.1, 1, 0.1],
                  scaleY: [1, 2, 1],
                }}
                transition={{ duration: 0.8, repeat: Infinity, delay: i * 0.1 }}
                className="w-1 h-full bg-[#4fb0c6]"
              />
            ))}
          </div>
        </div>
      </RollerBox>
    );
  }

  // 4. Result
  return (
    <RollerBox {...rollerProps}>
      <motion.div
        initial={{ opacity: 0, scale: 0.8 }}
        animate={{ opacity: 1, scale: 1 }}
        className={`text-[36px] font-black uppercase tracking-[0.3em] mb-6 drop-shadow-2xl ${outcome.color}`}
      >
        {outcome.label}
      </motion.div>

      <div className="w-full h-[1px] bg-white/10 mb-8" />

      <div className="space-y-6 font-serif w-full max-w-[200px]">
        <div className="flex justify-between items-center text-[15px]">
          <span className="text-gray-500 italic uppercase text-[10px] tracking-widest">
            Difficulty
          </span>
          <span className="text-white font-sans font-bold">{difficulty}</span>
        </div>

        <div className="relative h-[1px] flex items-center justify-center">
          <div className="w-full h-[1px] bg-white/10" />
          <div className="bg-[#050505] px-3 text-[10px] italic text-gray-600 relative z-10 uppercase tracking-tighter">
            Versus
          </div>
        </div>

        <div className="flex justify-between items-center text-[15px]">
          <span className="text-gray-500 italic uppercase text-[10px] tracking-widest">
            Your Total
          </span>
          <span className="text-white font-sans font-bold">{currentTotal}</span>
        </div>
      </div>

      <motion.div
        initial={{ opacity: 0, y: 10 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ delay: 0.3 }}
        className="mt-10 flex items-center gap-4 bg-white/[0.02] p-4 rounded-sm border border-white/5"
      >
        <div className="flex gap-2.5">
          {dice.map((v, i) => (
            <div
              key={i}
              className="w-10 h-10 rounded border border-white/10 bg-black/50 flex items-center justify-center shadow-inner"
            >
              <DieFace value={v} size="sm" />
            </div>
          ))}
        </div>
        <div className="h-8 w-[1px] bg-white/10" />
        <div className="flex flex-col items-center">
          <span className="text-[20px] font-black text-[#4fb0c6]">+{skillBonus}</span>
          <span className="text-[8px] text-gray-600 uppercase tracking-widest font-sans">Stat</span>
        </div>
      </motion.div>
    </RollerBox>
  );
};
