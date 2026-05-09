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

import { useEffect } from "react";
import type { Message, DialogueOption } from "@/types/dialogue";
import type { WorldSnapshot } from "@/types/entities";
import type { SseClient } from "@/services/SseClient";
import { worldManager } from "@/services/WorldManager";
import { nextId } from "@/client/idPool";
import { buildHistoryFromTree } from "@/client/historyUtils";

type TreeStepEntry = {
  id: string;
  parentStepId: string | null;
  parentOptionId: string | null;
  messages: Message[];
  options: DialogueOption[];
  worldSnapshot?: WorldSnapshot | null;
};

type TreeStepsMap = Record<string, TreeStepEntry>;

interface UseReplayModeParams {
  treeSteps: TreeStepsMap;
  setTreeSteps: React.Dispatch<React.SetStateAction<TreeStepsMap>>;
  history: Message[];
  setHistory: React.Dispatch<React.SetStateAction<Message[]>>;
  currentReplayStepId: string | null;
  setCurrentReplayStepId: React.Dispatch<React.SetStateAction<string | null>>;
  lastStepId: string | null;
  setLastStepId: React.Dispatch<React.SetStateAction<string | null>>;
  mode: "live" | "replay";
  setMode: React.Dispatch<React.SetStateAction<"live" | "replay">>;
  isRevealingRef: React.RefObject<boolean>;
  revealTimeoutRef: React.RefObject<ReturnType<typeof setTimeout> | null>;
  sseRef: React.RefObject<SseClient | null>;
  setDynamicOptions: React.Dispatch<React.SetStateAction<DialogueOption[] | null>>;
  setCanRegenerate: React.Dispatch<React.SetStateAction<boolean>>;
  setStreamingMessages: React.Dispatch<React.SetStateAction<Message[]>>;
  setIsTyping: React.Dispatch<React.SetStateAction<boolean>>;
  setHasBegun: React.Dispatch<React.SetStateAction<boolean>>;
  handleStreamingResponse: (
    userInput: string,
    updatedHistory: Message[],
    parentStepId: string | null,
    parentOptionId: string | null,
    onReplayDone?: (newStepId: string) => void,
  ) => Promise<void>;
}

export function useReplayMode({
  treeSteps,
  setTreeSteps,
  history,
  setHistory,
  currentReplayStepId,
  setCurrentReplayStepId,
  setLastStepId,
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
}: UseReplayModeParams) {
  // Cleanup reveal timeout on unmount
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
    await worldManager.loadState();
  };

  const handleReplayOptionSelect = async (option: DialogueOption) => {
    if (!currentReplayStepId) {
      console.warn(`[replay] no current step, cannot navigate`);
      return;
    }

    const youText = option.selectionMessage ?? option.text.replace(/^\[[^]]*?:[^]]*?\]\s*/, "");
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

    await handleStreamingResponse(
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
        await worldManager.loadState();
        console.log(`[replay] new branch saved: step=${newStepId}`);
      },
    );
  };

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

  return {
    enterReplayMode,
    exitReplayMode,
    handleReplayOptionSelect,
    handleJumpToStep,
    revealMessagesStaggered,
  };
}
