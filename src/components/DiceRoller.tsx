import React, { useState, useEffect, useRef } from "react";
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

// ── DieFace ─────────────────────────────────────────────────────────────────
// Exported — also used by DialogueMessage.tsx

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

  const s = sizes[size];
  return (
    <div className={`grid grid-cols-3 grid-rows-3 ${s.gap} ${s.container} pointer-events-none`}>
      {[...Array(9)].map((_, i) => (
        <div key={i} className="flex items-center justify-center">
          {dots.includes(i) && (
            <div
              className={`${s.dot} rounded-full bg-white shadow-[0_0_2px_rgba(255,255,255,0.5)]`}
            />
          )}
        </div>
      ))}
    </div>
  );
};

// ── DieBox ───────────────────────────────────────────────────────────────────

type DieVariant = "default" | "success" | "fail" | "critical" | "ghost";

const DieBox: React.FC<{
  value: number;
  size: "xs" | "sm" | "md" | "lg";
  variant?: DieVariant;
}> = ({ value, size, variant = "default" }) => {
  const sizeClass = {
    xs: "w-4 h-4 rounded-[2px]",
    sm: "w-8 h-8 rounded-[3px]",
    md: "w-14 h-14 rounded-[4px]",
    lg: "w-16 h-16 rounded-[4px]",
  }[size];

  const variantStyle: React.CSSProperties = {
    default: { background: "linear-gradient(135deg,#1c1c1c,#0b0b0b)", border: "1px solid rgba(255,255,255,0.12)" },
    success: { background: "linear-gradient(135deg,rgba(16,64,32,0.7),#0b0b0b)", border: "1px solid rgba(74,222,128,0.3)" },
    fail:    { background: "linear-gradient(135deg,rgba(64,12,12,0.7),#0b0b0b)", border: "1px solid rgba(248,113,113,0.3)" },
    critical:{ background: "linear-gradient(135deg,rgba(64,44,4,0.7),#0b0b0b)",  border: "1px solid rgba(245,158,11,0.4)" },
    ghost:   { background: "transparent", border: "1px solid rgba(255,255,255,0.06)", opacity: 0.2 },
  }[variant];

  return (
    <div
      className={`${sizeClass} flex items-center justify-center relative overflow-hidden`}
      style={variantStyle}
    >
      <div className="absolute inset-0 bg-gradient-to-br from-white/[0.04] to-transparent pointer-events-none" />
      <DieFace value={value} size={size} />
    </div>
  );
};

// ── ProbabilityArc ───────────────────────────────────────────────────────────

const ProbabilityArc: React.FC<{ probability: number; color: string }> = ({
  probability,
  color,
}) => {
  const r = 52;
  const circ = 2 * Math.PI * r;
  const offset = circ * (1 - probability / 100);

  return (
    <div className="relative flex items-center justify-center" style={{ width: 130, height: 130 }}>
      <svg width="130" height="130" className="absolute inset-0" style={{ transform: "rotate(-90deg)" }}>
        <defs>
          <filter id="arc-glow" x="-50%" y="-50%" width="200%" height="200%">
            <feGaussianBlur stdDeviation="3" result="blur" />
            <feMerge>
              <feMergeNode in="blur" />
              <feMergeNode in="SourceGraphic" />
            </feMerge>
          </filter>
        </defs>
        {/* Track */}
        <circle cx="65" cy="65" r={r} fill="none" stroke="rgba(255,255,255,0.04)" strokeWidth="2" />
        {/* Progress */}
        <motion.circle
          cx="65"
          cy="65"
          r={r}
          fill="none"
          stroke={color}
          strokeWidth="2.5"
          strokeDasharray={circ}
          initial={{ strokeDashoffset: circ }}
          animate={{ strokeDashoffset: offset }}
          transition={{ duration: 1.1, ease: "easeOut", delay: 0.15 }}
          strokeLinecap="round"
          filter="url(#arc-glow)"
        />
      </svg>
      <div className="relative z-10 flex flex-col items-center select-none">
        <span className="text-[44px] font-black leading-none tracking-tight" style={{ color }}>
          {probability}
        </span>
        <span className="text-[9px] text-white/20 uppercase tracking-[0.25em] font-bold mt-0.5">
          %
        </span>
      </div>
    </div>
  );
};

// ── Modal card wrapper ────────────────────────────────────────────────────────

const ModalCard: React.FC<{
  children: React.ReactNode;
  glowColor?: string;
  onClick?: () => void;
}> = ({ children, glowColor, onClick }) => (
  <motion.div
    initial={{ opacity: 0 }}
    animate={{ opacity: 1 }}
    exit={{ opacity: 0 }}
    className="fixed inset-0 z-[100] flex items-center justify-center p-4"
    style={{
      background: "radial-gradient(ellipse at 50% 40%, rgba(0,0,0,0.72) 0%, rgba(0,0,0,0.88) 100%)",
      backdropFilter: "blur(4px)",
    }}
  >
    <motion.div
      initial={{ opacity: 0, scale: 0.94, y: 14 }}
      animate={{ opacity: 1, scale: 1, y: 0 }}
      transition={{ type: "spring", stiffness: 290, damping: 26 }}
      onClick={onClick}
      className={`w-80 bg-[#060606] relative overflow-hidden${onClick ? " cursor-pointer" : ""}`}
      style={{
        boxShadow: glowColor
          ? `0 0 0 1px ${glowColor}28, 0 0 70px ${glowColor}14, 0 40px 100px rgba(0,0,0,0.85)`
          : "0 0 0 1px rgba(255,255,255,0.07), 0 40px 100px rgba(0,0,0,0.85)",
      }}
    >
      {/* Noise overlay */}
      <div className="absolute inset-0 pointer-events-none opacity-[0.025] bg-[radial-gradient(#fff_1px,transparent_1px)] [background-size:14px_14px]" />
      {/* Top edge highlight */}
      <div
        className="absolute inset-x-0 top-0 h-px pointer-events-none"
        style={{
          background: glowColor
            ? `linear-gradient(90deg,transparent,${glowColor}44,transparent)`
            : "linear-gradient(90deg,transparent,rgba(255,255,255,0.1),transparent)",
        }}
      />
      {children}
    </motion.div>
  </motion.div>
);

// ── Main component ────────────────────────────────────────────────────────────

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
  const completedRef = useRef(false);

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
          const finalDice = Array.from({ length: diceCount }, () => Math.floor(Math.random() * 6) + 1);
          setDice(finalDice);
          const diceTotal = finalDice.reduce((a, b) => a + b, 0);
          const total = diceTotal + skillBonus;
          setTimeout(() => {
            if (!completedRef.current) {
              completedRef.current = true;
              onComplete(total, total >= difficulty, finalDice);
            }
          }, 1900);
        }
      }, 400);
    }
    return () => clearInterval(interval);
  }, [isRolling, rollCount, skillBonus, difficulty, diceCount, onComplete]);

  const getOutcome = () => {
    const total = dice.reduce((a, b) => a + b, 0) + skillBonus;
    const success = total >= difficulty;

    if (conditions && hasRolled) {
      for (const cond of conditions) {
        try {
          const fn = new Function("dice", "total", "success", "diceLen", `return ${cond.expression}`);
          if (fn(dice, total, success, dice.length)) {
            return { label: cond.label ?? "Special Outcome", color: cond.color ?? "#a855f7", isSuccess: success };
          }
        } catch (e) {
          console.error("Condition eval error:", e);
        }
      }
    }

    return success
      ? { label: "Success", color: "#4ade80", isSuccess: true }
      : { label: "Failed", color: "#f87171", isSuccess: false };
  };

  const outcome = getOutcome();
  const diceTotal = dice.reduce((a, b) => a + b, 0);
  const currentTotal = diceTotal + skillBonus;

  const isNatural2 = hasRolled && dice.length === 2 && dice.every((d) => d === 1);
  const isNatural12 = hasRolled && dice.length === 2 && dice.every((d) => d === 6);

  // ── 1. Standard Idle ─────────────────────────────────────────────────────

  if (!isRolling && !hasRolled && !isRed) {
    return (
      <ModalCard>
        {/* Header */}
        <div className="px-5 py-2.5 bg-[#0e0e0e] flex items-center justify-between border-b border-white/[0.04]">
          <span className="text-[10px] font-black uppercase tracking-[0.3em] text-white/50">{skill}</span>
          <span className="text-[9px] uppercase tracking-[0.2em] text-white/20">Skill Check</span>
        </div>

        <div className="px-7 pt-8 pb-7 flex flex-col items-center gap-5">
          {/* Probability arc */}
          <ProbabilityArc probability={probability} color={probColor} />

          {/* Stat / difficulty row */}
          <div className="flex items-center gap-3 text-[10px] text-white/30 uppercase tracking-[0.15em] font-bold">
            <span>+{skillBonus} {skill}</span>
            <span className="w-0.5 h-0.5 rounded-full bg-white/15" />
            <span>{difficultyText ? `${difficultyText} ` : ""}Diff. {difficulty}</span>
          </div>

          {/* Ghost dice */}
          <div className="flex gap-3">
            {Array.from({ length: diceCount }, (_, i) => (
              <DieBox key={i} value={1} size="md" variant="ghost" />
            ))}
          </div>

          {/* CTA */}
          <button
            onClick={() => setIsRolling(true)}
            className="w-full py-2.5 mt-1 text-[10px] font-black uppercase tracking-[0.35em] text-white/30 border border-white/[0.08] hover:border-white/20 hover:text-white/60 hover:bg-white/[0.02] transition-all duration-200 cursor-pointer rounded-[1px]"
          >
            Roll the Dice
          </button>
        </div>
      </ModalCard>
    );
  }

  // ── 2. Red Check Idle ────────────────────────────────────────────────────

  if (!isRolling && !hasRolled && isRed) {
    return (
      <ModalCard glowColor="#dc2626">
        {/* Header */}
        <div className="px-5 py-2.5 bg-[#1a0606] flex items-center justify-between border-b border-red-900/30">
          <span className="text-[10px] font-black uppercase tracking-[0.3em] text-red-400">{skill}</span>
          <motion.span
            animate={{ opacity: [0.5, 1, 0.5] }}
            transition={{ duration: 2, repeat: Infinity, ease: "easeInOut" }}
            className="text-[9px] uppercase tracking-[0.25em] text-red-600 font-bold"
          >
            Red Check
          </motion.span>
        </div>

        <div className="px-7 pt-7 pb-7 flex flex-col items-center gap-5">
          {/* Warning label */}
          <div className="text-[9px] font-black tracking-[0.45em] uppercase text-red-800">
            ⬡ Irreversible ⬡
          </div>

          {/* Big pulsing probability */}
          <motion.div
            animate={{ opacity: [0.75, 1, 0.75] }}
            transition={{ duration: 2.2, repeat: Infinity, ease: "easeInOut" }}
            className="flex flex-col items-center"
          >
            <span
              className="text-[72px] font-black leading-none tracking-tighter"
              style={{ color: probColor, textShadow: `0 0 60px ${probColor}44` }}
            >
              {probability}%
            </span>
            <span className="text-[9px] text-white/15 uppercase tracking-[0.3em] mt-1">Probability</span>
          </motion.div>

          {/* Bar */}
          <div className="w-full h-[2px] bg-white/[0.04] overflow-hidden rounded-full">
            <motion.div
              initial={{ width: 0 }}
              animate={{ width: `${probability}%` }}
              transition={{ duration: 1.0, ease: "easeOut" }}
              className="h-full rounded-full"
              style={{ backgroundColor: probColor }}
            />
          </div>

          {/* Stat / difficulty */}
          <div className="flex items-center gap-3 text-[10px] text-white/25 uppercase tracking-[0.15em] font-bold">
            <span>+{skillBonus} {skill}</span>
            <span className="w-0.5 h-0.5 rounded-full bg-white/15" />
            <span>Difficulty {difficulty}</span>
          </div>

          {/* Flavor text */}
          <p className="text-[11px] font-serif italic text-white/20 text-center max-w-[200px] leading-relaxed">
            This moment will not come again. Failure will be absolute.
          </p>

          {/* Natural extremes */}
          <div className="flex w-full justify-between px-4 opacity-[0.18]">
            <div className="flex flex-col items-center gap-1.5">
              <div className="flex gap-1">
                {[1, 1].map((v, i) => <DieBox key={i} value={v} size="xs" />)}
              </div>
              <span className="text-[7px] uppercase tracking-widest text-white/40">Nat 2</span>
            </div>
            <div className="flex flex-col items-center gap-1.5">
              <div className="flex gap-1">
                {[6, 6].map((v, i) => <DieBox key={i} value={v} size="xs" />)}
              </div>
              <span className="text-[7px] uppercase tracking-widest text-white/40">Nat 12</span>
            </div>
          </div>

          {/* CTA */}
          <button
            onClick={() => setIsRolling(true)}
            className="w-full py-2.5 text-[10px] font-black uppercase tracking-[0.35em] text-red-600/70 border border-red-900/50 hover:border-red-700/80 hover:text-red-400 hover:bg-red-950/20 transition-all duration-200 cursor-pointer rounded-[1px]"
          >
            Commit
          </button>
        </div>
      </ModalCard>
    );
  }

  // ── 3. Rolling ───────────────────────────────────────────────────────────

  if (isRolling) {
    return (
      <ModalCard glowColor="#4fb0c6">
        {/* Header */}
        <div className="px-5 py-2.5 bg-[#071820] flex items-center justify-between border-b border-[#4fb0c6]/10">
          <span className="text-[10px] font-black uppercase tracking-[0.3em] text-[#4fb0c6]/70">{skill}</span>
          <span className="text-[9px] uppercase tracking-[0.2em] text-[#4fb0c6]/40">Rolling</span>
        </div>

        <div className="px-7 py-12 flex flex-col items-center gap-8">
          {/* Tumbling dice */}
          <div className="flex gap-5">
            {dice.map((value, i) => (
              <motion.div
                key={i}
                animate={{
                  rotate: i % 2 === 0 ? [0, 120, 240, 360] : [0, -120, -240, -360],
                  y: [0, -14, 0, -8, 0],
                  scale: [1, 1.06, 0.97, 1.03, 1],
                }}
                transition={{
                  duration: 0.4 + i * 0.07,
                  repeat: Infinity,
                  ease: "linear",
                }}
                className="w-14 h-14 bg-gradient-to-br from-[#1c1c1c] to-[#0a0a0a] border border-white/[0.14] rounded-[4px] flex items-center justify-center"
                style={{ boxShadow: "0 8px 24px rgba(0,0,0,0.7), inset 0 1px 0 rgba(255,255,255,0.04)" }}
              >
                <DieFace value={value} size="md" />
              </motion.div>
            ))}
          </div>

          {/* Calculating animation */}
          <div className="flex flex-col items-center gap-2.5">
            <span className="text-[8px] font-black tracking-[0.45em] uppercase text-[#4fb0c6]/50">
              Calculating
            </span>
            <div className="flex gap-[3px]">
              {Array.from({ length: 7 }, (_, i) => (
                <motion.div
                  key={i}
                  animate={{ opacity: [0.08, 1, 0.08], scaleY: [0.4, 2, 0.4] }}
                  transition={{ duration: 0.55, repeat: Infinity, delay: i * 0.08, ease: "easeInOut" }}
                  className="w-[2px] h-3 bg-[#4fb0c6] origin-center"
                />
              ))}
            </div>
          </div>
        </div>
      </ModalCard>
    );
  }

  // ── 4. Result ────────────────────────────────────────────────────────────

  const criticalLabel = isNatural12 ? "Critical Success" : isNatural2 ? "Critical Failure" : null;
  const resultColor = isNatural12 ? "#f59e0b" : isNatural2 ? "#ef4444" : outcome.color;
  const dieVariant: DieVariant = isNatural12 ? "critical" : isNatural2 ? "fail" : outcome.isSuccess ? "success" : "fail";

  const headerBg = isNatural12
    ? "#1e1200"
    : isNatural2
      ? "#160404"
      : outcome.isSuccess
        ? "#071510"
        : "#140606";

  const handleEarlyDismiss = () => {
    if (!completedRef.current) {
      completedRef.current = true;
      onComplete(currentTotal, outcome.isSuccess, dice);
    }
  };

  return (
    <ModalCard glowColor={resultColor} onClick={handleEarlyDismiss}>
      {/* Header */}
      <div
        className="px-5 py-2.5 flex items-center justify-between border-b border-white/[0.04] transition-colors duration-500"
        style={{ backgroundColor: headerBg }}
      >
        <span className="text-[10px] font-black uppercase tracking-[0.3em]" style={{ color: resultColor, opacity: 0.7 }}>
          {skill}
        </span>
        <span className="text-[9px] uppercase tracking-[0.2em]" style={{ color: resultColor, opacity: 0.45 }}>
          {outcome.isSuccess ? "Passed" : "Failed"}
        </span>
      </div>

      <div className="px-7 pt-7 pb-6 flex flex-col items-center gap-5">
        {/* Critical label */}
        {criticalLabel && (
          <motion.div
            initial={{ opacity: 0, y: -5 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ delay: 0.05 }}
            className="text-[9px] font-black uppercase tracking-[0.4em]"
            style={{ color: resultColor }}
          >
            ⬡ {criticalLabel} ⬡
          </motion.div>
        )}

        {/* Outcome word */}
        <motion.div
          initial={{ opacity: 0, scale: 0.72, y: 10 }}
          animate={{ opacity: 1, scale: 1, y: 0 }}
          transition={{ type: "spring", stiffness: 340, damping: 22, delay: 0.04 }}
          className="text-[48px] font-black uppercase tracking-[0.18em] leading-none"
          style={{ color: resultColor, textShadow: `0 0 60px ${resultColor}30` }}
        >
          {outcome.label}
        </motion.div>

        {/* Divider */}
        <motion.div
          initial={{ scaleX: 0 }}
          animate={{ scaleX: 1 }}
          transition={{ delay: 0.2, duration: 0.35, ease: "easeOut" }}
          className="w-full h-px bg-white/[0.06] origin-center"
        />

        {/* Total vs difficulty */}
        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={{ delay: 0.27 }}
          className="flex items-center gap-7"
        >
          <div className="flex flex-col items-center gap-0.5">
            <span className="text-[28px] font-black leading-none text-white">{currentTotal}</span>
            <span className="text-[8px] text-white/20 uppercase tracking-[0.2em]">Your Roll</span>
          </div>
          <span className="text-[11px] text-white/12 font-bold uppercase tracking-widest">vs</span>
          <div className="flex flex-col items-center gap-0.5">
            <span className="text-[28px] font-black leading-none text-white/30">{difficulty}</span>
            <span className="text-[8px] text-white/20 uppercase tracking-[0.2em]">Needed</span>
          </div>
        </motion.div>

        {/* Dice breakdown */}
        <motion.div
          initial={{ opacity: 0, y: 7 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ delay: 0.32 }}
          className="flex items-center gap-2.5 border border-white/[0.05] bg-white/[0.02] px-4 py-3 w-full justify-center rounded-[2px]"
        >
          {dice.map((v, i) => (
            <DieBox key={i} value={v} size="sm" variant={dieVariant} />
          ))}
          <span className="text-white/15 text-sm mx-0.5">+</span>
          <div className="flex flex-col items-center">
            <span className="text-[17px] font-black text-[#4fb0c6] leading-none">+{skillBonus}</span>
            <span className="text-[7px] text-white/20 uppercase tracking-widest mt-0.5">Stat</span>
          </div>
          <span className="text-white/15 text-sm mx-0.5">=</span>
          <div className="flex flex-col items-center">
            <span className="text-[17px] font-black leading-none" style={{ color: resultColor }}>
              {currentTotal}
            </span>
            <span className="text-[7px] text-white/20 uppercase tracking-widest mt-0.5">Total</span>
          </div>
        </motion.div>

        {/* Continue hint */}
        <motion.span
          animate={{ opacity: [0.15, 0.45, 0.15] }}
          transition={{ duration: 2.4, repeat: Infinity, ease: "easeInOut" }}
          className="text-[7px] font-bold tracking-[0.4em] uppercase text-white/30 mt-1"
        >
          Click to Continue
        </motion.span>
      </div>
    </ModalCard>
  );
};
