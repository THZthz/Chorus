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

import { useState, useEffect, useRef } from "react";
import { Trash2, RefreshCw, GitBranch, RotateCcw } from "lucide-react";
import { motion, AnimatePresence, LayoutGroup } from "motion/react";
import type { Message, DialogueOption } from "@/types/dialogue";
import type { WorldSnapshot, GameTime, SceneState } from "@/types/entities";
import type { SseClient } from "@/services/SseClient";
import { DialogueMessage } from "@/components/DialogueMessage";
import { DialogueOptions } from "@/components/DialogueOptions";
import { TypingIndicator } from "@/components/TypingIndicator";
// import { CharacterPanel } from "@/components/CharacterPanel";
import { DebugPanel } from "@/components/DebugPanel";
import { worldManager } from "@/services/WorldManager";
import { useCharacter } from "@/context/CharacterContext";
import { nextId, initIdPool } from "@/client/idPool";
import { SEGMENT_LABELS } from "@/shared/constants";
import { useSkillChecks } from "@/client/hooks/useSkillChecks";
import { useDialogueStreaming } from "@/client/hooks/useDialogueStreaming";
import { useReplayMode } from "@/client/hooks/useReplayMode";

export default function App() {
  const { character, getStatBySkillName } = useCharacter();

  // ── App-level state ──

  const [history, setHistory] = useState<Message[]>([]);
  const [hasBegun, setHasBegun] = useState(false);
  const [mode, setMode] = useState<"live" | "replay">("live");
  const [treeSteps, setTreeSteps] = useState<
    Record<
      string,
      {
        id: string;
        parentStepId: string | null;
        parentOptionId: string | null;
        messages: Message[];
        options: DialogueOption[];
        worldSnapshot?: WorldSnapshot | null;
      }
    >
  >({});
  const [currentReplayStepId, setCurrentReplayStepId] = useState<string | null>(null);
  const [regeneratingAll, setRegeneratingAll] = useState(false);
  const [gameTime, setGameTime] = useState<GameTime | null>(null);
  const [currentScene, setCurrentScene] = useState<SceneState | null>(null);

  // ── Refs ──

  const scrollContainerRef = useRef<HTMLDivElement>(null);
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const sseRef = useRef<SseClient | null>(null);
  const retrySnapshotRef = useRef<Message[]>([]);
  const isRetryingRef = useRef(false);
  const revealTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const isRevealingRef = useRef(false);

  // ── Custom hooks ──

  const { isRolling, handleSkillCheck } = useSkillChecks({ getStatBySkillName });

  const {
    isTyping,
    streamingMessages,
    dynamicOptions,
    canRegenerate,
    lastStepId,
    changedMessageIds,
    setLastStepId,
    setDynamicOptions,
    setCanRegenerate,
    setStreamingMessages,
    setIsTyping,
    handleStreamingResponse,
    handleRegenerate,
  } = useDialogueStreaming({
    character,
    sseRef,
    retrySnapshotRef,
    isRetryingRef,
    setHistory,
    setGameTime,
    setCurrentScene,
  });

  const { enterReplayMode, exitReplayMode, handleReplayOptionSelect, handleJumpToStep } =
    useReplayMode({
      treeSteps,
      setTreeSteps,
      history,
      setHistory,
      currentReplayStepId,
      setCurrentReplayStepId,
      lastStepId,
      setLastStepId,
      mode,
      setMode,
      isRevealingRef,
      revealTimeoutRef,
      sseRef,
      setDynamicOptions,
      setCanRegenerate,
      setStreamingMessages,
      setIsTyping,
      setHasBegun,
      handleStreamingResponse,
    });

  // ── Initial load ──

  useEffect(() => {
    async function init() {
      await initIdPool();
      await worldManager.loadState();
      const res = await fetch("/api/history");
      if (res.ok) {
        const hist = await res.json();
        if (hist.length > 0) {
          setHistory(hist);
          setHasBegun(true);

          const stepRes = await fetch("/api/session/current");
          if (stepRes.ok) {
            const step = await stepRes.json();
            if (step) {
              setLastStepId(step.id);
              setDynamicOptions(step.options ?? []);
              setCanRegenerate(true);
            } else {
              setDynamicOptions([]);
            }
          } else {
            setDynamicOptions([]);
          }
        }
      }
    }
    init();
  }, []);

  // ── Option selection ──

  const handleOptionSelect = async (option: DialogueOption) => {
    if (isTyping || isRolling || isRevealingRef.current) return;

    // Replay mode — navigate existing tree, no LLM
    if (mode === "replay") {
      handleReplayOptionSelect(option);
      return;
    }

    let updatedHistory = history;
    const youText = option.selectionMessage ?? option.text.replace(/^\[[^\]]*?:[^\]]*?\]\s*/, "");

    const youMessage: Message = {
      id: `you-${await nextId()}`,
      speaker: "YOU",
      type: "YOU",
      text: youText,
    };
    updatedHistory = [...history, youMessage];
    setHistory(updatedHistory);

    const handled = await handleSkillCheck(
      option,
      updatedHistory,
      (rollMessage, updatedHistoryWithRoll) => {
        const rr = rollMessage.rollResult!;
        const rc = option.check!;
        const resultLabel = rr.success ? "SUCCESS" : "FAILURE";

        const rollDescription = [
          `[Player action: ${youText}]`,
          `[Skill Check Result: ${rc.skill.toUpperCase()} (${rc.difficultyText})]`,
          `Rolled ${rr.dice.join(" + ")} + ${rr.skillBonus ?? 0} (${rc.skill}) = ${rr.total} vs Difficulty ${rc.difficulty}`,
          `Result: ${resultLabel}`,
        ].join("\n");

        setHistory(updatedHistoryWithRoll);
        handleStreamingResponse(rollDescription, updatedHistoryWithRoll, lastStepId, null);
      },
    );

    if (!handled) {
      setHasBegun(true);
      handleStreamingResponse(youText, updatedHistory, lastStepId, option.id);
    }
  };

  // ── Custom input ──

  const handleCustomInput = async (text: string) => {
    if (isTyping || isRolling || isRevealingRef.current) return;

    const youMessage: Message = {
      id: `you-${await nextId()}`,
      speaker: "YOU",
      type: "YOU",
      text,
    };
    const updatedHistory = [...history, youMessage];
    setHistory(updatedHistory);
    setHasBegun(true);
    handleStreamingResponse(text, updatedHistory, lastStepId, null);
  };

  // ── Reset ──

  const resetHistory = async () => {
    sseRef.current?.abort();
    setHistory([]);
    setDynamicOptions(null);
    setStreamingMessages([]);
    setCanRegenerate(false);
    setLastStepId(null);
    setHasBegun(false);
    await fetch("/api/reset", { method: "POST" });
    window.location.reload();
  };

  // ── Auto-scroll ──

  const scrollToBottom = () => {
    messagesEndRef.current?.scrollIntoView({ behavior: isTyping ? "auto" : "smooth" });
  };

  useEffect(() => {
    scrollToBottom();
  }, [history, isTyping, isRolling, streamingMessages]);

  // ── Begin story ──

  const handleBegin = () => {
    setHasBegun(true);
    handleStreamingResponse("[SYSTEM MESSAGE: Begin the story. Set the scene.]", [], null, null);
  };

  // ── Render ──

  return (
    <div className="h-screen w-screen bg-surface text-gray-100 flex justify-center selection:bg-accent/60 selection:text-white overflow-hidden relative">
      {/* Brass Frame */}
      <div className="fixed inset-0 pointer-events-none z-[100]">
        <div className="absolute inset-0 border-[3px] border-[#c4944a]/25 rounded-none" />
        <div className="absolute top-0 left-0 right-0 h-[6px] bg-gradient-to-b from-[#c4944a]/40 to-transparent" />
        <div className="absolute bottom-0 left-0 right-0 h-[6px] bg-gradient-to-t from-[#c4944a]/40 to-transparent" />
        <div className="absolute top-1.5 left-1.5 w-2 h-2 rounded-full bg-[#c4944a]/30" />
        <div className="absolute top-1.5 right-1.5 w-2 h-2 rounded-full bg-[#c4944a]/30" />
        <div className="absolute bottom-1.5 left-1.5 w-2 h-2 rounded-full bg-[#c4944a]/30" />
        <div className="absolute bottom-1.5 right-1.5 w-2 h-2 rounded-full bg-[#c4944a]/30" />
      </div>

      {/* Ground Glass Background */}
      <div className="fixed inset-0 pointer-events-none bg-ground-glass z-0" />

      {/* Lens Vignette Overlay */}
      <div
        className="fixed inset-0 pointer-events-none z-0"
        style={{
          background:
            "radial-gradient(ellipse 85% 80% at 50% 50%, transparent 30%, rgba(13,9,6,0.7) 75%, rgba(13,9,6,0.95) 100%)",
        }}
      />

      {/* <CharacterPanel /> */}

      {/* Time / Scene indicator */}
      {(gameTime || (mode === "replay" && worldManager.getGameTime())) && (
        <div className="fixed top-8 right-8 z-50 text-right pointer-events-none">
          <div className="text-xs text-white/40 font-mono tracking-wider">
            {(() => {
              const t = mode === "replay" ? worldManager.getGameTime() : gameTime;
              if (!t) return null;
              return `Day ${t.day} · ${SEGMENT_LABELS[t.segment] ?? `Segment ${t.segment}`}`;
            })()}
          </div>
          {currentScene && (
            <div className="text-xs text-white/25 font-mono mt-0.5 max-w-[200px] truncate">
              {currentScene.currentLocationId}
            </div>
          )}
        </div>
      )}

      {/* Action Controls */}
      <div className="fixed top-8 left-8 z-50 flex gap-3 items-center h-12">
        <LayoutGroup>
          <motion.button
            onClick={resetHistory}
            title="Reset Thought Stream"
            initial={{ color: "#6b7280", borderColor: "rgba(255, 255, 255, 0.05)" }}
            whileHover={{ scale: 1.1, color: "#ef4444", borderColor: "rgba(239, 68, 68, 0.5)" }}
            whileTap={{ scale: 0.95 }}
            className="h-11 w-11 flex-shrink-0 flex items-center justify-center bg-surface-card border rounded-full shadow-lg z-10"
          >
            <Trash2 size={18} />
          </motion.button>

          <AnimatePresence>
            {canRegenerate && !isTyping && (
              <motion.button
                key="regenerate"
                layout
                initial={{ opacity: 0, scale: 0.8 }}
                animate={{ opacity: 1, scale: 1 }}
                exit={{ opacity: 0, scale: 0.8 }}
                transition={{ type: "spring", stiffness: 500, damping: 45, mass: 0.5 }}
                onClick={() => handleRegenerate(history)}
                title="Regenerate Response"
                className="h-11 w-11 flex-shrink-0 flex items-center justify-center bg-surface-card border border-blue-400/30 rounded-full text-blue-400 hover:bg-blue-400 hover:text-white transition-all duration-300 shadow-xl"
              >
                <RefreshCw size={18} />
              </motion.button>
            )}
          </AnimatePresence>

          {/* Replay mode toggle */}
          <AnimatePresence>
            {mode === "live" && !isTyping && hasBegun && (
              <motion.button
                key="replay"
                layout
                initial={{ opacity: 0, scale: 0.8 }}
                animate={{ opacity: 1, scale: 1 }}
                exit={{ opacity: 0, scale: 0.8 }}
                transition={{ type: "spring", stiffness: 500, damping: 45, mass: 0.5 }}
                onClick={enterReplayMode}
                title="Replay Dialogue Tree"
                className="h-11 w-11 flex-shrink-0 flex items-center justify-center bg-surface-card border border-emerald-400/30 rounded-full text-emerald-400 hover:bg-emerald-400 hover:text-white transition-all duration-300 shadow-xl"
              >
                <GitBranch size={18} />
              </motion.button>
            )}
          </AnimatePresence>

          <AnimatePresence>
            {mode === "replay" && (
              <motion.button
                key="exit-replay"
                layout
                initial={{ opacity: 0, scale: 0.8 }}
                animate={{ opacity: 1, scale: 1 }}
                exit={{ opacity: 0, scale: 0.8 }}
                transition={{ type: "spring", stiffness: 500, damping: 45, mass: 0.5 }}
                onClick={exitReplayMode}
                title="Return to Live Mode"
                className="h-11 w-11 flex-shrink-0 flex items-center justify-center bg-surface-card border border-emerald-400/30 rounded-full text-emerald-400 hover:bg-emerald-400 hover:text-white transition-all duration-300 shadow-xl"
              >
                <RotateCcw size={18} />
              </motion.button>
            )}
          </AnimatePresence>
        </LayoutGroup>
      </div>

      {/* Background */}
      <div className="bg-texture" />
      <div className="fixed inset-0 pointer-events-none overflow-hidden">
        <div
          className="absolute inset-0 opacity-20"
          style={{
            backgroundImage: `radial-gradient(circle at 50% 50%, #444, #000)`,
            filter: "contrast(120%) brightness(80%)",
          }}
        />
        <div className="absolute inset-0 bg-gradient-to-t from-surface via-transparent to-transparent" />
      </div>

      {/* Main Content */}
      <main
        id="dialogue-scroll-container"
        ref={scrollContainerRef}
        className="relative w-full max-w-2xl h-full px-8 py-24 overflow-y-auto scroll-smooth no-scrollbar"
        style={{ scrollbarWidth: "none" }}
      >
        <div className="flex flex-col min-h-full">
          <div className="flex-1">
            {/* History messages */}
            {history.map((msg) => (
              <DialogueMessage
                key={msg.id}
                message={msg}
                isFlashing={changedMessageIds.has(msg.id)}
              />
            ))}

            {/* Streaming messages */}
            {streamingMessages.map((msg, idx) => (
              <div key={`stream-${msg.id}-${idx}`} className="mb-6 opacity-80">
                <DialogueMessage message={msg} isStreaming={idx === streamingMessages.length - 1} />
              </div>
            ))}

            {/* Typing indicator */}
            {isTyping && streamingMessages.length === 0 && <TypingIndicator />}
            <div ref={messagesEndRef} className="h-4" />
          </div>

          {/* Options */}
          <AnimatePresence mode="wait">
            {!hasBegun && !isTyping && history.length === 0 && mode === "live" && (
              <div className="mt-12 flex justify-center">
                <motion.button
                  key="begin"
                  initial={{ opacity: 0, y: 10 }}
                  animate={{ opacity: 1, y: 0 }}
                  transition={{ duration: 0.4 }}
                  onClick={handleBegin}
                  className="relative h-12 w-full max-w-md group"
                >
                  <div
                    className="absolute inset-0 bg-[#e63946] opacity-90 transition-colors group-hover:bg-[#ff4d5a]"
                    style={{
                      clipPath: "polygon(0% 0%, 98% 0%, 100% 50%, 98% 100%, 0% 100%)",
                      boxShadow: "inset 0 0 20px rgba(0,0,0,0.2)",
                    }}
                  />
                  <div className="absolute inset-0 flex items-center justify-between px-6">
                    <span className="text-white font-sans font-bold uppercase tracking-[0.3em] text-[16px]">
                      BEGIN
                    </span>
                    <motion.div
                      animate={{ x: [0, 5, 0] }}
                      transition={{ duration: 1.5, repeat: Infinity, ease: "easeInOut" }}
                      className="flex items-center"
                    >
                      <svg width="16" height="16" viewBox="0 0 24 24" fill="white">
                        <path d="M5 3L19 12L5 21V3Z" />
                      </svg>
                    </motion.div>
                  </div>
                  <div className="absolute inset-0 opacity-10 pointer-events-none mix-blend-overlay bg-[url('https://www.transparenttextures.com/patterns/dark-matter.png')]" />
                </motion.button>
              </div>
            )}
            {!isTyping && !isRolling && dynamicOptions && dynamicOptions.length > 0 && (
              <DialogueOptions
                key="dynamic"
                options={dynamicOptions}
                onSelect={handleOptionSelect}
                unexploredOptionIds={
                  mode === "replay"
                    ? new Set(dynamicOptions.filter((o) => !o.nextStepId).map((o) => o.id))
                    : undefined
                }
                onCustomInput={handleCustomInput}
                disabled={isTyping || isRolling}
              />
            )}
          </AnimatePresence>

          {/* Replay mode indicator */}
          {mode === "replay" && (
            <div className="text-center mt-4 mb-8">
              <span className="text-xs uppercase tracking-[0.3em] text-emerald-400/60 font-mono">
                Replay Mode — Navigate or Expand Tree
              </span>
            </div>
          )}

          <div className="h-32" />
        </div>
      </main>

      <DebugPanel onJumpToReplay={handleJumpToStep} currentReplayStepId={currentReplayStepId} />
      <div className="fixed left-0 top-0 bottom-0 w-2 bg-gradient-to-r from-black/50 to-transparent" />
      <div className="fixed right-0 top-0 bottom-0 w-2 bg-gradient-to-l from-black/50 to-transparent" />
    </div>
  );
}
