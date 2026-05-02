import { useState, useEffect, useRef } from "react";
import { Trash2, RefreshCw, GitBranch, RotateCcw } from "lucide-react";
import { motion, AnimatePresence, LayoutGroup } from "motion/react";
import type { Message, DialogueOption } from "@/types/dialogue";
import { DialogueMessage } from "@/components/DialogueMessage";
import { DialogueOptions } from "@/components/DialogueOptions";
import { TypingIndicator } from "@/components/TypingIndicator";
import { DiceRoller } from "@/components/DiceRoller";
import { CharacterPanel } from "@/components/CharacterPanel";
import { DebugPanel } from "@/components/DebugPanel";
import { worldManager } from "@/services/WorldManager";
import { SseClient } from "@/services/SseClient";
import { useCharacter } from "@/context/CharacterContext";

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
        const cleanText = opt.text.replace(/^\[[^\]]*?:[^\]]*?\]\s*/, "");
        result.push({ id: `you-tree-${i}`, speaker: "YOU", type: "YOU", text: cleanText });
      }
    }
    result.push(...step.messages);
  }
  return result;
}

export default function App() {
  const { character } = useCharacter();
  const [history, setHistory] = useState<Message[]>([]);
  const [isTyping, setIsTyping] = useState(false);
  const [currentCheck, setCurrentCheck] = useState<DialogueOption["check"] | null>(null);
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
        worldSnapshot?: Record<string, unknown> | null;
      }
    >
  >({});
  const [currentReplayStepId, setCurrentReplayStepId] = useState<string | null>(null);
  const [regeneratingAll, setRegeneratingAll] = useState(false);

  const scrollContainerRef = useRef<HTMLDivElement>(null);
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const sseRef = useRef<SseClient | null>(null);
  const retrySnapshotRef = useRef<Message[]>([]);
  const revealTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const isRevealingRef = useRef(false);

  // ── SSE streaming ──

  const handleStreamingResponse = (
    userInput: string,
    updatedHistory: Message[],
    parentStepId: string | null,
    parentOptionId: string | null,
    onReplayDone?: (newStepId: string) => void,
  ) => {
    console.log(
      `[stream] starting, parentStepId=${parentStepId} parentOptionId=${parentOptionId} historyLen=${updatedHistory.length} input="${String(userInput).slice(0, 60)}"`,
    );
    setIsTyping(true);
    setStreamingMessages([]);
    setDynamicOptions(null);
    setCanRegenerate(false);

    const streamId = `stream-${Date.now()}`;
    setStreamingId(streamId);

    let capturedStepId: string | null = null;

    const client = new SseClient();
    sseRef.current = client;

    client.stream(
      "/api/chat/stream",
      { userInput, history: updatedHistory, parentStepId, parentOptionId, playerCharacter: character },
      {
        onStepStart: (data) => {
          setLastStepId(data.stepId);
          capturedStepId = data.stepId;
          console.log(`[stream] step_start stepId=${data.stepId}`);
        },
        onStreamingMessages: (messages) => {
          setStreamingMessages(
            messages.map((m, i) => ({
              id: `${streamId}-${i}`,
              speaker: m.speaker,
              type: m.type as Message["type"],
              text: m.text,
              metadata: m.metadata as Message["metadata"],
            })),
          );
        },
        onStreamingReset: () => {
          // Snapshot current streaming messages so the retry can diff against them.
          // Keep them visible — don't wipe.
          setStreamingMessages((prev) => {
            retrySnapshotRef.current = prev;
            return prev;
          });
        },
        onOptions: (options) => {
          setDynamicOptions(options);
          console.log(`[stream] options received: ${options.length}`);
        },
        onParsed: (data) => {
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
          console.log(
            `[stream] parsed: ${messages.length} msgs, ${data.options?.length ?? 0} options, ${changed.size} changed`,
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
        onError: (message) => {
          console.error(`[stream] error: ${message}`);
          setIsTyping(false);
          setStreamingMessages([]);
          setHistory((prev) => [
            ...prev,
            {
              id: `error-${Date.now()}`,
              speaker: "SYSTEM",
              type: "SYSTEM",
              text: `[Error: ${message}]`,
            },
          ]);
        },
        onDone: () => {
          console.log(`[stream] done`);
          setIsTyping(false);
          setCanRegenerate(true);
          sseRef.current = null;
          worldManager.loadState();
          if (onReplayDone && capturedStepId) onReplayDone(capturedStepId);
        },
      },
    );
  };

  // ── Initial load ──

  useEffect(() => {
    async function init() {
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
    if (isTyping || currentCheck || isRevealingRef.current) return;

    // Replay mode — navigate existing tree, no LLM
    if (mode === "replay") {
      handleReplayOptionSelect(option);
      return;
    }

    let updatedHistory = history;
    const cleanText = option.text.replace(/^\[[^\]]*?:[^\]]*?\]\s*/, "");

    const youMessage: Message = {
      id: `you-${Date.now()}`,
      speaker: "YOU",
      type: "YOU",
      text: cleanText,
    };
    updatedHistory = [...history, youMessage];
    setHistory(updatedHistory);

    if (option.check) {
      setCurrentCheck(option.check);
    } else {
      setHasBegun(true);
      handleStreamingResponse(cleanText, updatedHistory, lastStepId, option.id);
    }
  };

  // ── Dice roll completion ──

  const handleRollComplete = async (total: number, success: boolean, dice: number[]) => {
    if (!currentCheck) return;

    const skillBonus = total - dice.reduce((a, b) => a + b, 0);
    const resultLabel = success ? "SUCCESS" : "FAILURE";

    // Build the user input describing the roll outcome
    const rollDescription = [
      `[Skill Check Result: ${currentCheck.skill.toUpperCase()} (${currentCheck.difficultyText})]`,
      `Rolled ${dice.join(" + ")} + ${skillBonus} (${currentCheck.skill}) = ${total} vs Difficulty ${currentCheck.difficulty}`,
      `Result: ${resultLabel}`,
    ].join("\n");

    setCurrentCheck(null);

    // Add a system notification about the roll
    const rollMessage: Message = {
      id: `roll-${Date.now()}`,
      speaker: "SYSTEM",
      type: "NOTIFICATION",
      text: `[${currentCheck.skill.toUpperCase()} - ${currentCheck.difficultyText} ${currentCheck.difficulty}] ${resultLabel} (${total} vs ${currentCheck.difficulty})`,
      rollResult: {
        dice,
        total,
        difficulty: currentCheck.difficulty,
        success,
        skill: currentCheck.skill,
        skillBonus,
      },
    };
    const updatedHistory = [...history, rollMessage];
    setHistory(updatedHistory);

    // Let the AI narrate the outcome
    handleStreamingResponse(rollDescription, updatedHistory, lastStepId, null);
  };

  // ── Regenerate ──

  const handleRegenerate = () => {
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

    const streamId = `stream-${Date.now()}`;
    setStreamingId(streamId);

    const client = new SseClient();
    sseRef.current = client;

    client.stream(
      "/api/regenerate",
      { stepId: lastStepId, history: trimmedHistory, playerCharacter: character },
      {
        onStepStart: (data) => {
          setLastStepId(data.stepId);
          console.log(`[regenerate] new stepId=${data.stepId}`);
        },
        onStreamingMessages: (messages) => {
          setStreamingMessages(
            messages.map((m, i) => ({
              id: `${streamId}-${i}`,
              speaker: m.speaker,
              type: m.type as Message["type"],
              text: m.text,
              metadata: m.metadata as Message["metadata"],
            })),
          );
        },
        onStreamingReset: () => {
          // Snapshot current streaming messages so the retry can diff against them.
          // Keep them visible — don't wipe.
          setStreamingMessages((prev) => {
            retrySnapshotRef.current = prev;
            return prev;
          });
        },
        onOptions: (options) => {
          setDynamicOptions(options);
        },
        onParsed: (data) => {
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
          console.log(
            `[regenerate] parsed ${messages.length} msgs, ${data.options?.length ?? 0} options, ${changed.size} changed`,
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
        onError: (message) => {
          console.error(`[regenerate] error: ${message}`);
          setIsTyping(false);
          setStreamingMessages([]);
          setHistory((prev) => [
            ...prev,
            {
              id: `error-${Date.now()}`,
              speaker: "SYSTEM",
              type: "SYSTEM",
              text: `[Error: ${message}]`,
            },
          ]);
        },
        onDone: () => {
          setIsTyping(false);
          setCanRegenerate(true);
          sseRef.current = null;
          worldManager.loadState();
          console.log(`[regenerate] done`);
        },
      },
    );
  };

  // ── Jump to specific step (from DialogueTreeGraph) ──

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

    const cleanText = option.text.replace(/^\[[^\]]*?:[^\]]*?\]\s*/, "");
    const youMessage: Message = {
      id: `you-${Date.now()}`,
      speaker: "YOU",
      type: "YOU",
      text: cleanText,
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
      cleanText,
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
              options: parent.options.map((o) =>
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

  // ── Bulk regenerate ──

  const handleBulkRegenerate = async () => {
    console.log(`[regenerate-all] starting bulk regenerate`);
    setRegeneratingAll(true);
    try {
      const res = await fetch("/api/regenerate-all", { method: "POST" });
      const data = await res.json();
      const succeeded = data.results?.filter((r: { success: boolean }) => r.success).length ?? 0;
      const total = data.results?.length ?? 0;
      console.log(`[regenerate-all] completed: ${succeeded}/${total} leaf steps regenerated`);
    } catch (err) {
      console.error("[regenerate-all] failed:", err);
    } finally {
      setRegeneratingAll(false);
      window.location.reload();
    }
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
  }, [history, isTyping, currentCheck, streamingMessages]);

  // ── Begin story ──

  const handleBegin = () => {
    setHasBegun(true);
    handleStreamingResponse("Begin the story. Set the scene.", [], null, null);
  };

  // ── Render ──

  return (
    <div className="h-screen w-screen bg-[#0a0a0a] text-gray-100 flex justify-center selection:bg-[#ff6b35] selection:text-white overflow-hidden relative">
      <CharacterPanel />

      {/* Decorative text */}
      <div className="fixed right-6 top-1/2 -translate-y-1/2 vertical-text text-[10px] uppercase tracking-[0.4em] text-white/10 font-mono hidden lg:block select-none pointer-events-none">
        LEFD &middot; B&Gamma;YAB &middot; SNAIO &middot; S&Gamma;A&Gamma;O
      </div>
      <div className="fixed left-6 top-1/2 -translate-y-1/2 vertical-text rotate-180 text-[10px] uppercase tracking-[0.4em] text-white/10 font-mono hidden lg:block select-none pointer-events-none">
        RHE&Gamma;ORIC &middot; LOGIC &middot; EMPA&Gamma;HY &middot; VISUAL CALCULUS
      </div>

      {/* Action Controls */}
      <div className="fixed top-8 left-8 z-50 flex gap-3 items-center h-12">
        <LayoutGroup>
          <motion.button
            onClick={resetHistory}
            title="Reset Thought Stream"
            initial={{ color: "#6b7280", borderColor: "rgba(255, 255, 255, 0.05)" }}
            whileHover={{ scale: 1.1, color: "#ef4444", borderColor: "rgba(239, 68, 68, 0.5)" }}
            whileTap={{ scale: 0.95 }}
            className="h-11 w-11 flex-shrink-0 flex items-center justify-center bg-[#1a1a1a] border rounded-full shadow-lg z-10"
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
                onClick={handleRegenerate}
                title="Regenerate Response"
                className="h-11 w-11 flex-shrink-0 flex items-center justify-center bg-[#1a1a1a] border border-blue-400/30 rounded-full text-blue-400 hover:bg-blue-400 hover:text-white transition-all duration-300 shadow-xl"
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
                className="h-11 w-11 flex-shrink-0 flex items-center justify-center bg-[#1a1a1a] border border-emerald-400/30 rounded-full text-emerald-400 hover:bg-emerald-400 hover:text-white transition-all duration-300 shadow-xl"
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
                className="h-11 w-11 flex-shrink-0 flex items-center justify-center bg-[#1a1a1a] border border-emerald-400/30 rounded-full text-emerald-400 hover:bg-emerald-400 hover:text-white transition-all duration-300 shadow-xl"
              >
                <RotateCcw size={18} />
              </motion.button>
            )}
          </AnimatePresence>

          {/* Bulk regenerate */}
          <AnimatePresence>
            {mode === "live" && canRegenerate && !isTyping && (
              <motion.button
                key="regenerate-all"
                layout
                initial={{ opacity: 0, scale: 0.8 }}
                animate={{ opacity: 1, scale: 1 }}
                exit={{ opacity: 0, scale: 0.8 }}
                transition={{ type: "spring", stiffness: 500, damping: 45, mass: 0.5 }}
                onClick={handleBulkRegenerate}
                disabled={regeneratingAll}
                title="Regenerate All Leaf Steps"
                className="h-11 w-11 flex-shrink-0 flex items-center justify-center bg-[#1a1a1a] border border-purple-400/30 rounded-full text-purple-400 hover:bg-purple-400 hover:text-white transition-all duration-300 shadow-xl disabled:opacity-50"
              >
                <RefreshCw size={18} className={regeneratingAll ? "animate-spin" : ""} />
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
        <div className="absolute inset-0 bg-gradient-to-t from-[#0a0a0a] via-transparent to-transparent" />
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

            {/* Dice roller modal */}
            <AnimatePresence>
              {currentCheck && <DiceRoller {...currentCheck} onComplete={handleRollComplete} />}
            </AnimatePresence>

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
            {!isTyping && !currentCheck && dynamicOptions && dynamicOptions.length > 0 && (
              <DialogueOptions
                key="dynamic"
                options={dynamicOptions}
                onSelect={handleOptionSelect}
                unexploredOptionIds={
                  mode === "replay"
                    ? new Set(dynamicOptions.filter((o) => !o.nextStepId).map((o) => o.id))
                    : undefined
                }
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
