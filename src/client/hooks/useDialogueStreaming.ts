import { useState, useEffect } from "react";
import type { Message, DialogueOption } from "@/types/dialogue";
import type { Character, GameTime, SceneState } from "@/types/entities";
import { SseClient, type SseCallbacks } from "@/services/SseClient";
import { worldManager } from "@/services/WorldManager";
import { nextId } from "@/client/idPool";

interface UseDialogueStreamingParams {
  character: Character;
  sseRef: React.MutableRefObject<SseClient | null>;
  retrySnapshotRef: React.MutableRefObject<Message[]>;
  isRetryingRef: React.MutableRefObject<boolean>;
  setHistory: React.Dispatch<React.SetStateAction<Message[]>>;
  setGameTime: React.Dispatch<React.SetStateAction<GameTime | null>>;
  setCurrentScene: React.Dispatch<React.SetStateAction<SceneState | null>>;
}

export function useDialogueStreaming({
  character,
  sseRef,
  retrySnapshotRef,
  isRetryingRef,
  setHistory,
  setGameTime,
  setCurrentScene,
}: UseDialogueStreamingParams) {
  const [isTyping, setIsTyping] = useState(false);
  const [streamingMessages, setStreamingMessages] = useState<Message[]>([]);
  const [dynamicOptions, setDynamicOptions] = useState<DialogueOption[] | null>(null);
  const [canRegenerate, setCanRegenerate] = useState(false);
  const [lastStepId, setLastStepId] = useState<string | null>(null);
  const [streamingId, setStreamingId] = useState<string | null>(null);
  const [changedMessageIds, setChangedMessageIds] = useState<Set<string>>(new Set());

  // Clear changedMessageIds after 900ms
  useEffect(() => {
    if (changedMessageIds.size === 0) return;
    const t = setTimeout(() => setChangedMessageIds(new Set()), 900);
    return () => clearTimeout(t);
  }, [changedMessageIds]);

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

  const handleRegenerate = async (history: Message[]) => {
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

  return {
    isTyping,
    setIsTyping,
    streamingMessages,
    setStreamingMessages,
    dynamicOptions,
    setDynamicOptions,
    canRegenerate,
    setCanRegenerate,
    lastStepId,
    setLastStepId,
    streamingId,
    changedMessageIds,
    handleStreamingResponse,
    handleRegenerate,
  };
}
