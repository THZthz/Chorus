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
import { DialogueMessage } from "@/components/DialogueMessage";
import { DialogueOptions } from "@/components/DialogueOptions";
import { TypingIndicator } from "@/components/TypingIndicator";
// import { CharacterPanel } from "@/components/CharacterPanel";
import { DebugPanel } from "@/components/DebugPanel";
import { worldManager } from "@/services/WorldManager";
import { SseClient, type SseCallbacks } from "@/services/SseClient";
import { useCharacter } from "@/context/CharacterContext";
import { nextId, initIdPool } from "@/client/idPool";
import { SEGMENT_LABELS } from "@/shared/constants";

function buildHistoryFromTree(
  stepId: string,
  treeSteps: Record<
    string,
    {
      id: string;
      parentStepId: string | null;
      parentOptionId: string | null;
      messages: Message[];
      options: DialogueOption[];
    }
  >,
): Message[] {
  const chain: (typeof treeSteps)[string][] = [];
  let cur: (typeof treeSteps)[string] | undefined = treeSteps[stepId];
  while (cur) {
    chain.unshift(cur);
    cur = cur.parentStepId ? treeSteps[cur.parentStepId] : undefined;
  }
  const result: Message[] = [];
  for (let i = 0; i < chain.length; i++) {
    const step = chain[i];
    if (i > 0) {
      const parent = chain[i - 1];
      const opt = parent.options.find((o) => o.id === step.parentOptionId);
      if (opt) {
        const youText = opt.selectionMessage ?? opt.text.replace(/^\[[^\]]*?:[^\]]*?\]\s*/, "");
        result.push({ id: `you-tree-${i}`, speaker: "YOU", type: "YOU", text: youText });
      } else if (!step.parentOptionId) {
        result.push({ id: `you-tree-${i}`, speaker: "YOU", type: "YOU", text: "[Free choice]" });
      }
    }
    result.push(...step.messages);
  }
  return result;
}

export default function App() {
  const { character, getStatBySkillName } = useCharacter();
  const [history, setHistory] = useState<Message[]>([]);
  const [isTyping, setIsTyping] = useState(false);
  const [isRolling, setIsRolling] = useState(false);
  const [dynamicOptions, setDynamicOptions] = useState<DialogueOption[] | null>(null);
  const [streamingMessages, setStreamingMessages] = useState<Message[]>([]);
  const [changedMessageIds, setChangedMessageIds] = useState<Set<string>>(new Set());
  const [canRegenerate, setCanRegenerate] = useState(false);
  const [lastStepId, setLastStepId] = useState<string | null>(null);
  const [hasBegun, setHasBegun] = useState(false);
  const [streamingId, setStreamingId] = useState<string | null>(null);

  // Replay mode state
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

  const scrollContainerRef = useRef<HTMLDivElement>(null);
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const sseRef = useRef<SseClient | null>(null);
  const retrySnapshotRef = useRef<Message[]>([]);
  const isRetryingRef = useRef(false);
  const revealTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const isRevealingRef = useRef(false);

  // ── SSE streaming ──

  const createSseCallbacks = (
    streamId: string,
    logPrefix: string,
    onDone?: (stepId: string | null) => void,
  ): SseCallbacks => {
    let capturedStepId: string | null = null;

    return {
      onStepStart: (data) => {
        setLastStepId(data.stepId);
        capturedStepId = data.stepId;
        console.trace(`[${logPrefix}] step_start stepId=${data.stepId}`);
      },
      onStreamingMessages: (messages) => {
        // During a retry, the LLM re-streams messages from scratch. Because
        // parsePartial produces different intermediate states across chunk
        // boundaries, content comparison is too fragile. Instead, freeze
        // the UI at the pre-reset state until parsed delivers the final result.
        if (isRetryingRef.current) return;

        setStreamingMessages((prev) => {
          if (
            prev.length === messages.length &&
            prev.every((m, i) => m.text === messages[i].text && m.speaker === messages[i].speaker)
          ) {
            return prev;
          }
          return messages.map((m, i) => ({
            id: `${streamId}-${i}`,
            speaker: m.speaker,
            type: m.type as Message["type"],
            text: m.text,
            metadata: m.metadata as Message["metadata"],
          }));
        });
      },
      onStreamingReset: () => {
        isRetryingRef.current = true;
        setStreamingMessages((prev) => {
          retrySnapshotRef.current = prev;
          return prev;
        });
      },
      onOptions: (options) => {
        setDynamicOptions(options);
        console.trace(`[${logPrefix}] options received: ${options.length}`);
      },
      onParsed: (data) => {
        isRetryingRef.current = false;
        const snapshot = retrySnapshotRef.current;
        retrySnapshotRef.current = [];
        const messages: Message[] = data.messages.map((m, i) => ({
          id: `${streamId}-final-${i}`,
          speaker: m.speaker,
          type: m.type as Message["type"],
          text: m.text,
          metadata: m.metadata as Message["metadata"],
        }));
        const changed = new Set(
          messages.filter((m, i) => (snapshot[i]?.text ?? null) !== m.text).map((m) => m.id),
        );
        setStreamingMessages([]);
        setHistory((prev) => [...prev, ...messages]);
        if (changed.size > 0) setChangedMessageIds(changed);
        if (data.options && data.options.length > 0) {
          setDynamicOptions(data.options);
        }
        console.trace(
          `[${logPrefix}] parsed: ${messages.length} msgs, ${data.options?.length ?? 0} options, ${changed.size} changed`,
        );
      },
      onWorldUpdate: () => {
        worldManager.loadState();
      },
      onPlotUpdate: () => {
        worldManager.loadState();
      },
      onPlotCreate: () => {
        worldManager.loadState();
      },
      onPlotEdit: () => {
        worldManager.loadState();
      },
      onTimeUpdate: (data) => {
        console.trace(`[${logPrefix}] time_update day=${data.day} segment=${data.segment}`);
        setGameTime({ day: data.day, segment: data.segment });
      },
      onSceneUpdate: (data) => {
        console.trace(`[${logPrefix}] scene_update`);
        setCurrentScene(data.scene);
      },
      onFactAdd: (data) => {
        console.trace(`[${logPrefix}] fact_add factId=${data.fact.id}`);
        worldManager.addFactToCache(data.fact);
      },
      onFactUpdate: (data) => {
        console.trace(`[${logPrefix}] fact_update factId=${data.factId}`);
        worldManager.updateFactInCache(data.factId, data.changes);
      },
      onFactRemove: (data) => {
        console.trace(`[${logPrefix}] fact_remove factId=${data.factId}`);
        worldManager.removeFactFromCache(data.factId);
      },
      onError: async (message) => {
        isRetryingRef.current = false;
        console.error(`[${logPrefix}] error: ${message}`);
        setIsTyping(false);
        setStreamingMessages([]);
        const errorId = `error-${await nextId()}`;
        setHistory((prev) => [
          ...prev,
          {
            id: errorId,
            speaker: "SYSTEM",
            type: "SYSTEM",
            text: `[Error: ${message}]`,
          },
        ]);
      },
      onDone: () => {
        console.trace(`[${logPrefix}] done`);
        setIsTyping(false);
        setCanRegenerate(true);
        sseRef.current = null;
        worldManager.loadState();
        onDone?.(capturedStepId);
      },
    };
  };

  const handleStreamingResponse = async (
    userInput: string,
    updatedHistory: Message[],
    parentStepId: string | null,
    parentOptionId: string | null,
    onReplayDone?: (newStepId: string) => void,
  ) => {
    console.trace(
      `[stream] starting, parentStepId=${parentStepId} parentOptionId=${parentOptionId} historyLen=${updatedHistory.length} input="${String(userInput).slice(0, 60)}"`,
    );
    setIsTyping(true);
    setStreamingMessages([]);
    setDynamicOptions(null);
    setCanRegenerate(false);
    isRetryingRef.current = false;

    const streamId = `stream-${await nextId()}`;
    setStreamingId(streamId);

    const client = new SseClient();
    sseRef.current = client;

    client.stream(
      "/api/chat/stream",
      {
        userInput,
        history: updatedHistory,
        parentStepId,
        parentOptionId,
        playerCharacter: character,
      },
      createSseCallbacks(streamId, "stream", (stepId) => {
        if (onReplayDone && stepId) onReplayDone(stepId);
      }),
    );
  };

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

    if (option.check) {
      setIsRolling(true);
      const check = option.check;
      const diceCount = check.diceCount ?? 2;
      await new Promise((r) => setTimeout(r, 1000));
      const dice = Array.from({ length: diceCount }, () => Math.floor(Math.random() * 6) + 1);
      const skillBonus = getStatBySkillName(check.skill);
      const total = dice.reduce((a, b) => a + b, 0) + skillBonus;
      const success = total >= check.difficulty;
      handleRollComplete(check, total, success, dice, skillBonus, updatedHistory, youText);
    } else {
      setHasBegun(true);
      handleStreamingResponse(youText, updatedHistory, lastStepId, option.id);
    }
  };

  // ── Dice roll completion ──

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

  const handleRollComplete = async (
    check: NonNullable<DialogueOption["check"]>,
    total: number,
    success: boolean,
    dice: number[],
    skillBonus: number,
    historySnapshot: Message[],
    optionText: string,
  ) => {
    const resultLabel = success ? "SUCCESS" : "FAILURE";

    const rollDescription = [
      `[Player action: ${optionText}]`,
      `[Skill Check Result: ${check.skill.toUpperCase()} (${check.difficultyText})]`,
      `Rolled ${dice.join(" + ")} + ${skillBonus} (${check.skill}) = ${total} vs Difficulty ${check.difficulty}`,
      `Result: ${resultLabel}`,
    ].join("\n");

    setIsRolling(false);

    const rollMessage: Message = {
      id: `roll-${await nextId()}`,
      speaker: "SYSTEM",
      type: "NOTIFICATION",
      text: optionText,
      skillCheck: {
        skill: check.skill,
        difficulty: `${check.difficultyText} ${check.difficulty}`,
        success,
      },
      rollResult: {
        dice,
        total,
        difficulty: check.difficulty,
        success,
        skill: check.skill,
        skillBonus,
      },
    };
    const updatedHistory = [...historySnapshot, rollMessage];
    setHistory(updatedHistory);

    handleStreamingResponse(rollDescription, updatedHistory, lastStepId, null);
  };

  // ── Regenerate ──

  const handleRegenerate = async () => {
    if (!lastStepId || isTyping) return;
    sseRef.current?.abort();

    // Trim history to last YOU message (compute before setState to avoid stale closure)
    const lastYouIdx = history.map((m) => m.type).lastIndexOf("YOU");
    const trimmedHistory = lastYouIdx >= 0 ? history.slice(0, lastYouIdx + 1) : history;

    console.log(
      `[regenerate] triggering, stepId=${lastStepId} historyLen=${trimmedHistory.length}`,
    );

    setHistory(trimmedHistory);
    setDynamicOptions(null);
    setStreamingMessages([]);
    setIsTyping(true);

    const streamId = `stream-${await nextId()}`;
    setStreamingId(streamId);

    const client = new SseClient();
    sseRef.current = client;

    client.stream(
      "/api/regenerate",
      { stepId: lastStepId, history: trimmedHistory, playerCharacter: character },
      createSseCallbacks(streamId, "regenerate"),
    );
  };

  // ── Jump to specific step (from DebugPanel dialogue tree) ──

  const handleJumpToStep = async (stepId: string) => {
    sseRef.current?.abort();
    setIsTyping(false);
    setStreamingMessages([]);
    setCanRegenerate(false);

    const treeRes = await fetch("/api/dialogue/tree");
    if (!treeRes.ok) return;

    const treeData = await treeRes.json();
    const targetStep = treeData.steps[stepId];
    if (!targetStep) return;

    // Use buildHistoryFromTree so YOU messages are injected between steps
    const messages = buildHistoryFromTree(stepId, treeData.steps);

    setTreeSteps(treeData.steps);
    setHistory(messages);
    setDynamicOptions(targetStep.options);
    setCurrentReplayStepId(stepId);
    setLastStepId(stepId);
    setCanRegenerate(true);
    setHasBegun(true);
    setMode("replay");
    worldManager.applyStepSnapshot(targetStep.worldSnapshot);
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

  // ── Replay mode ──

  const enterReplayMode = async () => {
    sseRef.current?.abort();
    setIsTyping(false);
    setStreamingMessages([]);
    setCanRegenerate(false);

    console.log(`[replay] entering replay mode, fetching tree...`);
    const res = await fetch("/api/dialogue/tree");
    if (!res.ok) {
      console.error(`[replay] tree fetch failed: ${res.status}`);
      return;
    }
    const data = await res.json();
    if (!data.root) {
      console.warn(`[replay] no root step found in tree`);
      return;
    }

    const stepCount = Object.keys(data.steps).length;
    console.log(
      `[replay] tree loaded: root=${data.root.id}, steps=${stepCount}, leaves=${data.leafIds?.length ?? 0}, totalMsgs=${data.root.messages?.length ?? 0}, options=${data.root.options?.length ?? 0}`,
    );

    setTreeSteps(data.steps);
    setCurrentReplayStepId(data.root.id);
    setHistory(data.root.messages);
    setDynamicOptions(data.root.options);
    setHasBegun(true);
    setMode("replay");
    worldManager.applyStepSnapshot(data.steps[data.root.id]?.worldSnapshot);
  };

  const exitReplayMode = async () => {
    console.log(`[replay] exiting replay mode, restoring live history`);
    setMode("live");
    setTreeSteps({});
    setCurrentReplayStepId(null);
    worldManager.clearReplayState();

    const res = await fetch("/api/history");
    if (res.ok) {
      const hist = await res.json();
      if (hist.length > 0) {
        setHistory(hist);
        setHasBegun(true);
        setDynamicOptions([]);
      } else {
        setHistory([]);
        setHasBegun(false);
        setDynamicOptions(null);
      }
    }
    worldManager.loadState();
  };

  const handleReplayOptionSelect = async (option: DialogueOption) => {
    if (!currentReplayStepId) {
      console.warn(`[replay] no current step, cannot navigate`);
      return;
    }

    const youText = option.selectionMessage ?? option.text.replace(/^\[[^\]]*?:[^\]]*?\]\s*/, "");
    const youMessage: Message = {
      id: `you-${await nextId()}`,
      speaker: "YOU",
      type: "YOU",
      text: youText,
    };

    console.log(
      `[replay] option selected: id=${option.id} nextStepId=${option.nextStepId || "none"} text="${String(option.text).slice(0, 40)}"`,
    );

    // Fast path — child already in local treeSteps
    if (option.nextStepId && treeSteps[option.nextStepId]) {
      const child = treeSteps[option.nextStepId];
      console.log(`[replay] fast-path navigate to step=${child.id}`);
      const baseWithYou = [...history, youMessage];
      setHistory(baseWithYou);
      revealMessagesStaggered(baseWithYou, child.messages, () => {
        setDynamicOptions(child.options);
        setCurrentReplayStepId(child.id);
        setLastStepId(child.id);
        setCanRegenerate(true);
        worldManager.applyStepSnapshot(child.worldSnapshot);
      });
      return;
    }

    // Slow path — nextStepId exists but not in local cache; check server
    if (option.nextStepId) {
      console.log(`[replay] slow-path lookup: stepId=${currentReplayStepId} optionId=${option.id}`);
      const res = await fetch("/api/dialogue/traverse", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ stepId: currentReplayStepId, optionId: option.id }),
      });
      if (res.ok) {
        const { child } = await res.json();
        if (child) {
          console.log(`[replay] slow-path found child step=${child.id}`);
          setTreeSteps((prev) => ({ ...prev, [child.id]: child }));
          const baseWithYou = [...history, youMessage];
          setHistory(baseWithYou);
          revealMessagesStaggered(baseWithYou, child.messages, () => {
            setDynamicOptions(child.options);
            setCurrentReplayStepId(child.id);
            setLastStepId(child.id);
            setCanRegenerate(true);
            worldManager.applyStepSnapshot(child.worldSnapshot);
          });
          return;
        }
      }
    }

    // New branch — no child exists yet, generate one
    console.log(
      `[replay] generating new branch from step=${currentReplayStepId} option=${option.id}`,
    );
    setHistory((prev) => [...prev, youMessage]);
    const branchHistory = buildHistoryFromTree(currentReplayStepId, treeSteps);
    const updatedHistory = [...branchHistory, youMessage];
    const parentIdAtTime = currentReplayStepId;

    handleStreamingResponse(
      youText,
      updatedHistory,
      currentReplayStepId,
      option.id,
      async (newStepId) => {
        const stepRes = await fetch(`/api/dialogue/${newStepId}`);
        if (!stepRes.ok) return;
        const { step } = await stepRes.json();
        setTreeSteps((prev) => {
          const updated = { ...prev, [newStepId]: step };
          const parent = updated[parentIdAtTime];
          if (parent) {
            updated[parentIdAtTime] = {
              ...parent,
              options: parent.options.map((o: any) =>
                o.id === option.id ? { ...o, nextStepId: newStepId } : o,
              ),
            };
          }
          return updated;
        });
        setCurrentReplayStepId(newStepId);
        // New branch used live world state — clear override so CharacterPanel shows live state
        worldManager.loadState();
        console.log(`[replay] new branch saved: step=${newStepId}`);
      },
    );
  };

  useEffect(() => {
    if (changedMessageIds.size === 0) return;
    const t = setTimeout(() => setChangedMessageIds(new Set()), 900);
    return () => clearTimeout(t);
  }, [changedMessageIds]);

  // ── Staggered message reveal (replay navigation) ──

  useEffect(() => {
    return () => {
      if (revealTimeoutRef.current) clearTimeout(revealTimeoutRef.current);
    };
  }, []);

  const revealMessagesStaggered = (
    baseMessages: Message[],
    newMessages: Message[],
    onDone: () => void,
  ) => {
    if (revealTimeoutRef.current) clearTimeout(revealTimeoutRef.current);
    isRevealingRef.current = true;
    setDynamicOptions([]);

    let revealed = 0;
    const revealNext = () => {
      if (revealed >= newMessages.length) {
        isRevealingRef.current = false;
        onDone();
        return;
      }
      setHistory([...baseMessages, ...newMessages.slice(0, revealed + 1)]);
      revealed++;
      revealTimeoutRef.current = setTimeout(revealNext, 120);
    };
    revealNext();
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
          background: "radial-gradient(ellipse 85% 80% at 50% 50%, transparent 30%, rgba(13,9,6,0.7) 75%, rgba(13,9,6,0.95) 100%)",
        }}
      />

      {/* <CharacterPanel /> */}


      {/* Top Rail */}
      <div className="fixed top-0 left-0 right-0 z-50 h-10 flex items-center justify-between px-6 pointer-events-none">
        {/* Left: Time + Scene */}
        <div className="flex items-center gap-4">
          {(gameTime || (mode === "replay" && worldManager.getGameTime())) && (
            <>
              <span className="text-engraved text-[10px]">
                {(() => {
                  const t = mode === "replay" ? worldManager.getGameTime() : gameTime;
                  if (!t) return null;
                  return `DAY ${t.day} · ${SEGMENT_LABELS[t.segment] ?? `SEG ${t.segment}`}`;
                })()}
              </span>
              {currentScene && (
                <>
                  <span className="text-[#c4944a]/30 text-[10px]">◆</span>
                  <span className="text-engraved text-[10px] opacity-60">
                    {currentScene.currentLocationId}
                  </span>
                </>
              )}
              {(mode === "replay" || hasBegun) && (
                <>
                  <span className="text-[#c4944a]/30 text-[10px]">◆</span>
                  <span className="text-[#d4786c]/60 text-[8px] font-mono uppercase tracking-[0.3em]">
                    {mode === "replay" ? "RECORDED" : "LIVE"}
                  </span>
                </>
              )}
            </>
          )}
        </div>

        {/* Right: Action Toggles */}
        <div className="flex items-center gap-2 pointer-events-auto">
          <LayoutGroup>
            {/* Reset */}
            <motion.button
              onClick={resetHistory}
              title="Reset"
              whileHover={{ scale: 1.08 }}
              whileTap={{ scale: 0.92 }}
              className="h-8 w-8 flex items-center justify-center brass-ring rounded-full text-[#c4944a]/60 hover:text-[#d4786c] hover:border-[#d4786c]/50 transition-colors duration-200"
            >
              <Trash2 size={14} />
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
                  onClick={handleRegenerate}
                  title="Regenerate Response"
                  className="h-8 w-8 flex items-center justify-center brass-ring rounded-full text-[#c4944a]/60 hover:text-[#e0b460] hover:border-[#e0b460]/50 transition-colors duration-200"
                >
                  <RefreshCw size={14} />
                </motion.button>
              )}
            </AnimatePresence>

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
                  className="h-8 w-8 flex items-center justify-center brass-ring rounded-full text-[#7ec8e0]/60 hover:text-[#7ec8e0] hover:border-[#7ec8e0]/50 transition-colors duration-200"
                >
                  <GitBranch size={14} />
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
                  className="h-8 w-8 flex items-center justify-center brass-ring rounded-full text-[#8fbc8f]/60 hover:text-[#8fbc8f] hover:border-[#8fbc8f]/50 transition-colors duration-200"
                >
                  <RotateCcw size={14} />
                </motion.button>
              )}
            </AnimatePresence>

            {/* Debug iris toggle */}
            <span className="text-[#c4944a]/15 text-[10px] mx-1">|</span>
            <motion.button
              onClick={() => {
                const debugBtn = document.getElementById("debug-panel-toggle");
                debugBtn?.click();
              }}
              title="Debug Tools"
              whileHover={{ scale: 1.08 }}
              whileTap={{ scale: 0.92 }}
              className="h-8 w-8 flex items-center justify-center brass-ring rounded-full text-[#c4944a]/40 hover:text-[#c4944a] hover:border-[#c4944a]/50 transition-colors duration-200"
            >
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                <circle cx="12" cy="12" r="3" />
                <path d="M12 1v4M12 19v4M4.22 4.22l2.83 2.83M16.95 16.95l2.83 2.83M1 12h4M19 12h4M4.22 19.78l2.83-2.83M16.95 7.05l2.83-2.83" />
              </svg>
            </motion.button>
          </LayoutGroup>
        </div>
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
    </div>
  );
}
