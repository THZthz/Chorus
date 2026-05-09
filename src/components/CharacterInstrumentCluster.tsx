import React, { useState, useEffect } from "react";
import { motion } from "motion/react";
import { useCharacter } from "@/context/CharacterContext";
import { worldManager } from "@/services/WorldManager";

interface GaugeProps {
  name: string;
  value: number;
  color: string;
  index: number;
  visible: boolean;
}

const CharacterGauge: React.FC<GaugeProps> = ({ name, value, color, index, visible }) => (
  <motion.div
    initial={visible ? { opacity: 0, scale: 0.3 } : false}
    animate={visible ? { opacity: 1, scale: 1 } : { opacity: 0, scale: 0.3 }}
    transition={{ delay: index * 0.05, duration: 0.15, ease: "easeOut" }}
    className="flex flex-col items-center gap-0.5"
    title={`${name}: ${value}`}
  >
    <div
      className="relative w-9 h-9 rounded-full flex items-center justify-center"
      style={{
        border: `2px solid rgba(196,148,74,0.25)`,
        boxShadow: "inset 0 1px 3px rgba(0,0,0,0.4)",
      }}
    >
      <div
        className="absolute inset-0 rounded-full"
        style={{
          background: `conic-gradient(${color} ${(value / 6) * 360}deg, transparent ${(value / 6) * 360}deg)`,
          opacity: 0.15,
        }}
      />
      <span
        className="relative text-[11px] font-bold z-10"
        style={{
          fontFamily: "'Barlow Condensed', 'Arial Narrow', sans-serif",
          color: "#e8dcc8",
          textShadow: "0 1px 0 rgba(0,0,0,0.5)",
        }}
      >
        {value}
      </span>
    </div>
    <span
      className="text-[6px] uppercase tracking-[0.2em] text-[#c4944a]/50"
      style={{ fontFamily: "'Barlow Condensed', 'Arial Narrow', sans-serif" }}
    >
      {name.length > 8 ? name.slice(0, 7) + "…" : name}
    </span>
  </motion.div>
);

export const CharacterInstrumentCluster: React.FC = () => {
  const { character: liveCharacter } = useCharacter();
  const [, forceUpdate] = useState(0);
  const [gaugesVisible, setGaugesVisible] = useState(false);

  useEffect(() => worldManager.subscribe(() => forceUpdate((n) => n + 1)), []);

  useEffect(() => {
    const t = setTimeout(() => setGaugesVisible(true), 100);
    return () => clearTimeout(t);
  }, []);

  const character = worldManager.getPlayerCharacter() ?? liveCharacter;
  const entries = Object.entries(character.stats) as [string, number][];

  const getColor = (name: string): string => {
    const colors: Record<string, string> = {
      LOGIC: "#5ca8b8",
      RHETORIC: "#b8a45a",
      EMPATHY: "#b86878",
      PERCEPTION: "#5ab898",
      VOLITION: "#c87848",
      ENDURANCE: "#b05858",
      SORCERY: "#8878d0",
      SUGGESTION: "#98b85a",
      INSTINCT: "#c86060",
      MIGHT: "#5ab868",
      CLOCKWORK: "#5aa8b8",
      ALCHEMY: "#8fdf8f",
    };
    return colors[name] ?? "#c4944a";
  };

  return (
    <div className="flex items-center gap-3 px-4">
      {entries.map(([name, value], i) => (
        <CharacterGauge
          key={name}
          name={name}
          value={value}
          color={getColor(name)}
          index={i}
          visible={gaugesVisible}
        />
      ))}
    </div>
  );
};
