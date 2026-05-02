import React, { useState, useEffect } from "react";
import { motion } from "motion/react";
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

const calculate2D6Probability = (target: number, bonus: number) => {
  const neededOnDice = target - bonus;
  if (neededOnDice <= 2) return 100;
  if (neededOnDice > 12) return 0;
  let successes = 0;
  for (let d1 = 1; d1 <= 6; d1++) {
    for (let d2 = 1; d2 <= 6; d2++) {
      if (d1 + d2 >= neededOnDice) successes++;
    }
  }
  return Math.round((successes / 36) * 100);
};

const probabilityColor = (p: number) =>
  p >= 75 ? "#4ade80" : p >= 50 ? "#facc15" : p >= 30 ? "#fb923c" : "#f87171";

export const DieFace: React.FC<{ value: number; size?: "sm" | "md" | "lg" | "xs" }> = ({
  value,
  size = "md",
}) => {
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

const RollerBox = ({
  children,
  onClick,
  isRolling,
  hasRolled,
  outcome,
  skill,
  difficultyText,
}: {
  children: React.ReactNode;
  onClick?: () => void;
  isRolling: boolean;
  hasRolled: boolean;
  outcome: { label: string; color: string; isSuccess: boolean };
  skill: string;
  difficultyText: string;
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
        className={`w-80 bg-[#050505] border border-white/10 shadow-[0_0_100px_rgba(0,0,0,1)] overflow-hidden rounded-sm relative transition-all duration-500 ${onClick ? "cursor-pointer" : ""}`}
        onClick={onClick}
      >
        <div className="absolute inset-0 opacity-[0.03] pointer-events-none bg-[radial-gradient(#fff_1px,transparent_1px)] [background-size:16px_16px]" />

        {/* Header */}
        <div
          className={`px-4 py-2.5 font-sans font-bold uppercase tracking-[0.2em] text-center text-[11px] transition-colors duration-500 ${
            isRolling
              ? "bg-[#4fb0c6] text-white"
              : hasRolled
                ? outcome.isSuccess
                  ? "bg-[#2d5a27] text-white"
                  : "bg-[#5a2727] text-white"
                : "bg-[#1a1a1a] text-white/70"
          }`}
        >
          {skill} Check
          {!isRolling && !hasRolled && difficultyText && (
            <span className="ml-1.5 opacity-40 normal-case font-normal tracking-wide">
              — {difficultyText}
            </span>
          )}
          {hasRolled && (
            <span className="ml-1.5 opacity-60 font-normal tracking-wide">
              — {outcome.isSuccess ? "Passed" : "Failed"}
            </span>
          )}
        </div>

        <div className="p-8 flex flex-col items-center text-center relative z-10 min-h-[340px] justify-center">
          {children}
        </div>

        {/* Always-visible pulsing click prompt */}
        {!isRolling && !hasRolled && onClick && (
          <motion.div
            animate={{ opacity: [0.2, 0.5, 0.2] }}
            transition={{ duration: 2.2, repeat: Infinity, ease: "easeInOut" }}
            className="absolute inset-x-0 bottom-4 flex justify-center pointer-events-none"
          >
            <span className="text-white text-[8px] font-bold tracking-[0.35em] uppercase">
              Click to Proceed
            </span>
          </motion.div>
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
  const probColor = probabilityColor(probability);

  useEffect(() => {
    let interval: NodeJS.Timeout;
    if (isRolling) {
      interval = setInterval(() => {
        setDice((prev) => prev.map(() => Math.floor(Math.random() * 6) + 1));
        setRollCount((c) => c + 1);

        if (rollCount > 6) {
          setIsRolling(false);
          setHasRolled(true);
          const finalDice = new Array(diceCount)
            .fill(0)
            .map(() => Math.floor(Math.random() * 6) + 1);
          setDice(finalDice);
          const diceTotal = finalDice.reduce((a, b) => a + b, 0);
          const totalWithBonus = diceTotal + skillBonus;

          setTimeout(() => {
            onComplete(totalWithBonus, totalWithBonus >= difficulty, finalDice);
          }, 1800);
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

  const isNatural2 = hasRolled && dice.length === 2 && dice.every((d) => d === 1);
  const isNatural12 = hasRolled && dice.length === 2 && dice.every((d) => d === 6);

  const rollerProps = { isRolling, hasRolled, outcome, skill, difficultyText };

  // 1. Red Check Idle
  if (isRed && !isRolling && !hasRolled) {
    return (
      <RollerBox {...rollerProps} onClick={() => setIsRolling(true)}>
        <motion.div
          animate={{ scale: [1, 1.015, 1], opacity: [0.85, 1, 0.85] }}
          transition={{ duration: 2.5, repeat: Infinity, ease: "easeInOut" }}
          className="text-[#ff4d4d] text-[18px] uppercase tracking-[0.4em] font-black mb-2 drop-shadow-[0_0_12px_rgba(255,77,77,0.4)]"
        >
          Red Check
        </motion.div>

        <div
          className="text-[64px] font-bold text-white leading-none mb-3 tracking-tighter"
          style={{ textShadow: `0 0 40px ${probColor}55` }}
        >
          {probability}%
        </div>

        <div className="w-36 h-[3px] bg-white/5 rounded-full overflow-hidden mb-6">
          <motion.div
            initial={{ width: 0 }}
            animate={{ width: `${probability}%` }}
            transition={{ duration: 0.9, ease: "easeOut" }}
            className="h-full rounded-full"
            style={{ backgroundColor: probColor }}
          />
        </div>

        <div className="text-gray-400 text-[12px] font-serif italic mb-8 max-w-[220px] leading-relaxed">
          This action is irreversible. Failure will be absolute.
        </div>

        <div className="flex justify-between w-full px-4">
          <div className="flex flex-col items-center gap-1.5 opacity-25">
            <div className="flex gap-1">
              {[1, 1].map((v, i) => (
                <div
                  key={i}
                  className="w-6 h-6 bg-white/10 rounded-sm flex items-center justify-center border border-white/5"
                >
                  <DieFace value={v} size="xs" />
                </div>
              ))}
            </div>
            <span className="text-[7px] uppercase tracking-widest text-gray-500">Natural 2</span>
          </div>

          <div className="flex items-center">
            <span className="text-[9px] uppercase tracking-widest text-white/15">vs</span>
          </div>

          <div className="flex flex-col items-center gap-1.5 opacity-25">
            <div className="flex gap-1">
              {[6, 6].map((v, i) => (
                <div
                  key={i}
                  className="w-6 h-6 bg-white/10 rounded-sm flex items-center justify-center border border-white/5"
                >
                  <DieFace value={v} size="xs" />
                </div>
              ))}
            </div>
            <span className="text-[7px] uppercase tracking-widest text-gray-500">Natural 12</span>
          </div>
        </div>
      </RollerBox>
    );
  }

  // 2. Standard Check Idle
  if (!isRolling && !hasRolled) {
    return (
      <RollerBox {...rollerProps} onClick={() => setIsRolling(true)}>
        <div
          className="text-[13px] uppercase tracking-[0.4em] font-bold mb-3 opacity-60"
          style={{ color: probColor }}
        >
          Probability
        </div>
        <div className="text-[72px] font-bold text-white leading-none mb-2 tracking-tighter">
          {probability}%
        </div>

        <div className="w-full max-w-[160px] h-1 bg-white/5 rounded-full overflow-hidden mb-6 mt-2">
          <motion.div
            initial={{ width: 0 }}
            animate={{ width: `${probability}%` }}
            transition={{ duration: 0.9, ease: "easeOut" }}
            className="h-full rounded-full"
            style={{ backgroundColor: probColor }}
          />
        </div>

        <div className="flex gap-4 items-center text-[11px] font-serif italic text-gray-400">
          <span>
            +{skillBonus} {skill}
          </span>
          <span className="w-1 h-1 rounded-full bg-white/20" />
          <span>
            {difficultyText || "Difficulty"} {difficulty}
          </span>
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
              animate={{ rotate: 360, scale: [1, 1.08, 1], y: [0, -10, 0] }}
              transition={{
                rotate: { duration: 0.3 + i * 0.08, repeat: Infinity, ease: "linear" },
                scale: { duration: 0.5, repeat: Infinity, ease: "easeInOut", delay: i * 0.2 },
                y: { duration: 0.5, repeat: Infinity, ease: "easeInOut", delay: i * 0.2 },
              }}
              className="w-16 h-16 bg-[#111] border border-white/20 rounded shadow-[0_0_30px_rgba(255,255,255,0.05)] flex items-center justify-center"
            >
              <DieFace value={value} size="md" />
            </motion.div>
          ))}
        </div>

        <div className="flex flex-col items-center gap-3">
          <div className="text-[10px] font-black text-[#4fb0c6] tracking-[0.4em] uppercase">
            Calculating Outcome
          </div>
          <div className="flex gap-1.5 h-1">
            {[...Array(5)].map((_, i) => (
              <motion.div
                key={i}
                animate={{ opacity: [0.1, 1, 0.1], scaleY: [1, 2, 1] }}
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
  const criticalLabel = isNatural12 ? "Critical Success" : isNatural2 ? "Critical Failure" : null;
  const diceBoxClass = isNatural12
    ? "border-amber-400/50 bg-amber-900/20"
    : isNatural2
      ? "border-red-600/50 bg-red-900/20"
      : "border-white/10 bg-black/50";
  const breakdownBorderClass = isNatural12
    ? "border-amber-400/30 shadow-[0_0_30px_rgba(251,191,36,0.08)]"
    : isNatural2
      ? "border-red-600/30 shadow-[0_0_30px_rgba(220,38,38,0.08)]"
      : "border-white/5";

  return (
    <RollerBox {...rollerProps}>
      {criticalLabel && (
        <motion.div
          initial={{ opacity: 0, y: -6 }}
          animate={{ opacity: 1, y: 0 }}
          className={`text-[10px] font-black uppercase tracking-[0.35em] mb-3 ${
            isNatural12 ? "text-amber-400" : "text-red-400"
          }`}
        >
          {criticalLabel}
        </motion.div>
      )}

      <motion.div
        initial={{ opacity: 0, scale: 0.82 }}
        animate={{ opacity: 1, scale: 1 }}
        transition={{ type: "spring", stiffness: 300, damping: 22 }}
        className={`text-[32px] font-black uppercase tracking-[0.25em] mb-6 drop-shadow-2xl ${
          isNatural12 ? "text-amber-400" : isNatural2 ? "text-red-500" : outcome.color
        }`}
      >
        {outcome.label}
      </motion.div>

      <div className="w-full h-[1px] bg-white/10 mb-6" />

      <div className="space-y-4 font-serif w-full max-w-[200px]">
        <div className="flex justify-between items-center">
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

        <div className="flex justify-between items-center">
          <span className="text-gray-500 italic uppercase text-[10px] tracking-widest">
            Your Total
          </span>
          <span className="text-white font-sans font-bold">{currentTotal}</span>
        </div>
      </div>

      <motion.div
        initial={{ opacity: 0, y: 8 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ delay: 0.25 }}
        className={`mt-8 flex items-center gap-3 bg-white/[0.02] p-3.5 rounded-sm border transition-all ${breakdownBorderClass}`}
      >
        <div className="flex gap-2">
          {dice.map((v, i) => (
            <div
              key={i}
              className={`w-11 h-11 rounded border flex items-center justify-center shadow-inner ${diceBoxClass}`}
            >
              <DieFace value={v} size="sm" />
            </div>
          ))}
        </div>
        <div className="h-8 w-[1px] bg-white/10" />
        <div className="flex flex-col items-center min-w-[28px]">
          <span className="text-[18px] font-black text-[#4fb0c6]">+{skillBonus}</span>
          <span className="text-[7px] text-gray-600 uppercase tracking-widest font-sans">Stat</span>
        </div>
        <div className="h-8 w-[1px] bg-white/10" />
        <div className="flex flex-col items-center min-w-[28px]">
          <span className="text-[18px] font-black text-white">{currentTotal}</span>
          <span className="text-[7px] text-gray-600 uppercase tracking-widest font-sans">
            Total
          </span>
        </div>
      </motion.div>
    </RollerBox>
  );
};
