import React, { useState, useEffect, useRef } from 'react';
import { FastForward, Trash2, RefreshCw } from 'lucide-react';
import { motion, AnimatePresence, LayoutGroup, useScroll, useTransform } from 'motion/react';
import { Message, DialogueStep, DialogueOption } from '@/types/dialogue';
import { sampleDialogue } from '@/data/sampleDialogue';
import { DialogueMessage } from '@/components/DialogueMessage';
import { DialogueOptions } from '@/components/DialogueOptions';
import { TypingIndicator } from '@/components/TypingIndicator';
import { DiceRoller } from '@/components/DiceRoller';
import { CharacterPanel } from '@/components/CharacterPanel';
import { DebugPanel } from '@/components/DebugPanel';
import { worldManager } from '@/services/WorldManager';
import { SseClient } from '@/services/SseClient';

export default function App() {
  const [history, setHistory] = useState<Message[]>([]);
  const [currentStepId, setCurrentStepId] = useState<string>('start');
  const [isTyping, setIsTyping] = useState(false);
  const [currentCheck, setCurrentCheck] = useState<DialogueOption['check'] | null>(null);
  const [dynamicOptions, setDynamicOptions] = useState<DialogueOption[] | null>(null);
  const [isFastForward, setIsFastForward] = useState(false);
  const [streamingText, setStreamingText] = useState<string | null>(null);
  const [streamingMessages, setStreamingMessages] = useState<Message[]>([]);
  const [streamingId, setStreamingId] = useState<string | null>(null);
  const [canRegenerate, setCanRegenerate] = useState(false);
  const [lastStepId, setLastStepId] = useState<string | null>(null);

  const isFastForwardRef = useRef(false);
  const scrollContainerRef = useRef<HTMLDivElement>(null);
  const scrollBarRef = useRef<HTMLDivElement>(null);
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const sseRef = useRef<SseClient | null>(null);

  const { scrollYProgress } = useScroll({
    container: scrollContainerRef,
  });

  const dotTop = useTransform(scrollYProgress, [0, 1], ["0%", "100%"]);

  const handleScrollbarDrag = (_: any, info: any) => {
    if (!scrollBarRef.current || !scrollContainerRef.current) return;
    const container = scrollContainerRef.current;
    const barRect = scrollBarRef.current.getBoundingClientRect();
    const progress = Math.max(0, Math.min(1, (info.point.y - barRect.top) / barRect.height));
    container.scrollTop = progress * (container.scrollHeight - container.clientHeight);
  };

  const handleBarClick = (e: React.MouseEvent) => {
    if (!scrollBarRef.current || !scrollContainerRef.current) return;
    const barRect = scrollBarRef.current.getBoundingClientRect();
    const progress = Math.max(0, Math.min(1, (e.clientY - barRect.top) / barRect.height));
    const container = scrollContainerRef.current;
    container.scrollTo({
      top: progress * (container.scrollHeight - container.clientHeight),
      behavior: 'smooth'
    });
  };

  const currentStep = sampleDialogue[currentStepId];

  // Handle streaming response
  const handleStreamingResponse = (userInput: string, updatedHistory: Message[], parentStepId: string | null, parentOptionId: string | null) => {
    setIsTyping(true);
    setStreamingText("");
    setStreamingMessages([]);
    setDynamicOptions(null);
    setCanRegenerate(false);

    const streamId = `stream-${Date.now()}`;
    setStreamingId(streamId);

    const client = new SseClient();
    sseRef.current = client;

    client.stream('/api/chat/stream', {
      userInput,
      history: updatedHistory,
      parentStepId,
      parentOptionId,
    }, {
      onToken: (token) => {
        setStreamingText(prev => (prev ?? "") + token);
      },
      onStreamingMessages: (messages) => {
        setStreamingMessages(messages.map((m, i) => ({
          ...m,
          id: `${streamId}-${i}`,
          type: m.type as Message['type']
        })));
      },
      onWorldUpdate: () => {
        worldManager.loadState();
      },
      onPlotUpdate: () => {
        // Plot state refreshed implicitly
      },
      onParsed: (data) => {
        // Replace streaming text with structured messages
        setStreamingText(null);
        setStreamingMessages([]);
        const messages: Message[] = data.messages.map((m, i) => ({
          id: `${streamId}-${i}`,
          speaker: m.speaker,
          type: m.type as Message['type'],
          text: m.text,
        }));
        setHistory(prev => [...prev, ...messages]);
        if (data.options && data.options.length > 0) {
          setDynamicOptions(data.options);
        }
      },
      onOptions: (options) => {
        setDynamicOptions(options);
      },
      onStepStart: (data) => {
        setLastStepId(data.stepId);
      },
      onError: (message) => {
        setStreamingText(null);
        setIsTyping(false);
        setHistory(prev => [...prev, {
          id: `error-${Date.now()}`,
          speaker: 'SYSTEM',
          type: 'SYSTEM',
          text: `[Error: ${message}]`
        }]);
      },
      onDone: () => {
        setIsTyping(false);
        setCanRegenerate(true);
        sseRef.current = null;
        worldManager.loadState();
      },
    });
  };

  // Auto-scroll to bottom
  const scrollToBottom = () => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  };

  useEffect(() => {
    scrollToBottom();
  }, [history, isTyping, currentCheck, streamingText]);

  // Handle sequential message display for static content
  const displayMessages = async (messages: Message[]) => {
    setIsTyping(true);
    setIsFastForward(false);
    isFastForwardRef.current = false;

    for (const msg of messages) {
      if (!isFastForwardRef.current) {
        const delay = Math.min(Math.max(msg.text.length * 20, 1000), 3000);
        const startTime = Date.now();
        while (Date.now() - startTime < delay && !isFastForwardRef.current) {
          await new Promise(resolve => setTimeout(resolve, 50));
        }
      } else {
        await new Promise(resolve => setTimeout(resolve, 50));
      }
      setHistory(prev => [...prev, msg]);
    }

    setIsTyping(false);
    setIsFastForward(false);
    isFastForwardRef.current = false;
  };

  const resetHistory = async () => {
    sseRef.current?.abort();
    setHistory([]);
    setCurrentStepId('start');
    setDynamicOptions(null);
    setStreamingText(null);
    setStreamingMessages([]);
    setCanRegenerate(false);
    setLastStepId(null);
    await fetch('/api/reset', { method: 'POST' });
    window.location.reload();
  };

  useEffect(() => {
    async function loadData() {
      await worldManager.loadState();
      const res = await fetch('/api/history');
      if (res.ok) {
        const hist = await res.json();
        if (hist.length > 0) {
          setHistory(hist);
          initializedRef.current = true;
          setDynamicOptions([]);
        }
      }
    }
    loadData();
  }, []);

  const initializedRef = useRef(false);

  useEffect(() => {
    if (currentStep && !initializedRef.current && history.length === 0) {
      initializedRef.current = true;
      const initialMessages = currentStep.messages.map((m, i) => ({
        ...m,
        id: `initial-${i}-${Math.random().toString(36).substr(2, 9)}`
      }));
      displayMessages(initialMessages);
    }
  }, [history.length, currentStep]);

  // Check for pre-generated children before streaming
  const checkPreGenerated = async (option: DialogueOption): Promise<boolean> => {
    if (!lastStepId) return false;
    try {
      const res = await fetch(`/api/dialogue/${lastStepId}/children`);
      if (res.ok) {
        const children = await res.json();
        if (children && children.length > 0) {
          const match = children.find((c: { parentOptionId: string }) => c.parentOptionId === option.id);
          if (match) {
            // Load pre-generated step
            const stepRes = await fetch(`/api/dialogue/${match.id}`);
            if (stepRes.ok) {
              const { step } = await stepRes.json();
              setLastStepId(step.id);
              setCanRegenerate(true);
              const messages = step.messages.map((m: Message, i: number) => ({
                ...m,
                id: m.id || `pregen-${match.id}-${i}`,
              }));
              setHistory(prev => [...prev, ...messages]);
              setDynamicOptions(step.options);
              // Activate this branch
              fetch('/api/branches/activate', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ stepId: step.id, parentStepId: step.parentStepId }),
              });
              return true;
            }
          }
        }
      }
    } catch {
      // Fall through to streaming
    }
    return false;
  };

  const handleOptionSelect = async (option: DialogueOption) => {
    if (isTyping || currentCheck) return;

    let updatedHistory = history;
    const cleanText = option.text.replace(/^\[[^\]]*?:[^\]]*?\]\s*/, '');

    if (!option.isContinue) {
      const youMessage: Message = {
        id: `you-${Date.now()}`,
        speaker: 'YOU',
        type: 'YOU',
        text: cleanText
      };
      updatedHistory = [...history, youMessage];
      setHistory(updatedHistory);
    }

    if (option.check) {
      setCurrentCheck(option.check);
    } else if (option.nextStepId) {
      setDynamicOptions(null);
      const nextStep = sampleDialogue[option.nextStepId];
      if (!nextStep) return;
      const nextMessages: Message[] = nextStep.messages.map((m, i) => ({
        ...m,
        id: `${option.nextStepId}-${i}-${Date.now()}`
      }));
      setCurrentStepId(option.nextStepId);
      await displayMessages(nextMessages);
    } else if (option.isAiTrigger || dynamicOptions) {
      // Try pre-generated first
      const preGenHit = await checkPreGenerated(option);
      if (!preGenHit) {
        handleStreamingResponse(cleanText, updatedHistory, lastStepId, option.id);
      }
    }
  };

  const handleRegenerate = () => {
    if (!lastStepId || isTyping) return;
    sseRef.current?.abort();

    // Remove last AI messages from history
    setHistory(prev => {
      const lastYouIdx = prev.map(m => m.type).lastIndexOf('YOU');
      if (lastYouIdx >= 0) {
        return prev.slice(0, lastYouIdx + 1);
      }
      return prev;
    });
    setDynamicOptions(null);
    setStreamingText(null);
    setStreamingMessages([]);

    const lastYouMsg = history.filter(m => m.type === 'YOU').pop();
    const userInput = lastYouMsg?.text ?? 'Continue';

    handleStreamingResponse(userInput, history, null, null);
  };

  const handleRollComplete = async (total: number, success: boolean, dice: number[]) => {
    if (!currentCheck) return;

    let outcomeStepId: string | null = null;
    const skillBonus = total - dice.reduce((a, b) => a + b, 0);

    for (const condition of currentCheck.conditions) {
      try {
        const evaluator = new Function('dice', 'total', 'success', 'diceLen', `return ${condition.expression}`);
        if (evaluator(dice, total, success, dice.length)) {
          outcomeStepId = condition.stepId;
          break;
        }
      } catch (e) {
        console.error('Error evaluating roll condition:', e);
      }
    }

    if (!outcomeStepId) {
      outcomeStepId = success ? 'start' : 'start';
    }

    const nextStep = sampleDialogue[outcomeStepId];

    const rollData = {
      dice,
      total,
      success,
      difficulty: currentCheck.difficulty,
      skill: currentCheck.skill,
      skillBonus
    };

    setCurrentCheck(null);
    setCurrentStepId(outcomeStepId);

    if (nextStep) {
      const nextMessages: Message[] = nextStep.messages.map((m, i) => ({
        ...m,
        id: `${outcomeStepId}-${i}-${Date.now()}`,
        rollResult: i === 0 && m.skillCheck ? rollData : undefined
      }));
      await displayMessages(nextMessages);
    }
  };

  return (
    <div className="h-screen w-screen bg-[#0a0a0a] text-gray-100 flex justify-center selection:bg-[#ff6b35] selection:text-white overflow-hidden relative">
      <CharacterPanel />

      {/* Decorative Side Elements */}
      <div
        ref={scrollBarRef}
        onClick={handleBarClick}
        className="fixed right-12 top-0 bottom-0 w-[40px] hidden sm:flex justify-center cursor-pointer z-40 group"
      >
        <div className="w-[1px] h-full bg-white/10 group-hover:bg-white/20 transition-colors" />
        <div className="absolute top-1/4 h-24 w-[1px] bg-gradient-to-b from-transparent via-white/40 to-transparent" />
        <motion.div
          drag="y"
          dragConstraints={scrollBarRef}
          dragElastic={0}
          dragMomentum={false}
          onDrag={handleScrollbarDrag}
          className="absolute w-2.5 h-2.5 rounded-full bg-white shadow-[0_0_12px_rgba(255,255,255,0.6)] cursor-grab active:cursor-grabbing hover:scale-125 transition-transform"
          style={{
            top: dotTop,
            y: "-50%"
          }}
        />
        <div className="absolute top-8 left-1/2 -translate-x-1/2 w-3 h-[1px] bg-white/40" />
        <div className="absolute top-8 left-1/2 -translate-x-1/2 h-4 w-[1px] -translate-y-full flex flex-col items-center">
          <div className="w-[1px] h-full bg-white/20" />
          <div className="w-2 h-2 border-t border-r border-white/20 rotate-[-45deg] -translate-y-1" />
        </div>
      </div>

      <div className="fixed right-6 top-1/2 -translate-y-1/2 vertical-text text-[10px] uppercase tracking-[0.4em] text-white/10 font-mono hidden lg:block select-none pointer-events-none">
        LEFD • BΓYAB • SNAIO • SΓAΓO
      </div>

      <div className="fixed left-6 top-1/2 -translate-y-1/2 vertical-text rotate-180 text-[10px] uppercase tracking-[0.4em] text-white/10 font-mono hidden lg:block select-none pointer-events-none">
        RHEΓORIC • LOGIC • EMPAΓHY • VISUAL CALCULUS
      </div>

      {/* Action Controls */}
      <div className="fixed top-8 left-8 z-50 flex gap-3 items-center h-12">
        <LayoutGroup>
          <motion.button
            onClick={resetHistory}
            title="Reset Thought Stream"
            initial={{ color: '#6b7280', borderColor: 'rgba(255, 255, 255, 0.05)' }}
            whileHover={{ scale: 1.1, color: '#ef4444', borderColor: 'rgba(239, 68, 68, 0.5)' }}
            whileTap={{ scale: 0.95 }}
            className="h-11 w-11 flex-shrink-0 flex items-center justify-center bg-[#1a1a1a] border rounded-full shadow-lg z-10"
          >
            <Trash2 size={18} />
          </motion.button>

          <AnimatePresence>
            {isTyping && (
              <motion.button
                key="fast-forward-button"
                layout
                initial={{ opacity: 0, scale: 0.8 }}
                animate={{ opacity: 1, scale: 1 }}
                exit={{ opacity: 0, scale: 0.8 }}
                transition={{
                  type: 'spring',
                  stiffness: 500,
                  damping: 45,
                  mass: 0.5
                }}
                onClick={() => {
                  isFastForwardRef.current = true;
                  setIsFastForward(true);
                }}
                title="Fast Forward"
                className="h-11 w-11 flex-shrink-0 flex items-center justify-center bg-[#1a1a1a] border border-[#ff6b35]/30 rounded-full text-[#ff6b35] hover:bg-[#ff6b35] hover:text-white transition-all duration-300 shadow-xl"
              >
                <FastForward size={18} />
              </motion.button>
            )}
          </AnimatePresence>

          <AnimatePresence>
            {canRegenerate && !isTyping && (
              <motion.button
                key="regenerate-button"
                layout
                initial={{ opacity: 0, scale: 0.8 }}
                animate={{ opacity: 1, scale: 1 }}
                exit={{ opacity: 0, scale: 0.8 }}
                transition={{
                  type: 'spring',
                  stiffness: 500,
                  damping: 45,
                  mass: 0.5
                }}
                onClick={handleRegenerate}
                title="Regenerate Response"
                className="h-11 w-11 flex-shrink-0 flex items-center justify-center bg-[#1a1a1a] border border-blue-400/30 rounded-full text-blue-400 hover:bg-blue-400 hover:text-white transition-all duration-300 shadow-xl"
              >
                <RefreshCw size={18} />
              </motion.button>
            )}
          </AnimatePresence>
        </LayoutGroup>
      </div>

      {/* Moody background overlay */}
      <div className="bg-texture" />
      <div className="fixed inset-0 pointer-events-none overflow-hidden">
        <div
          className="absolute inset-0 opacity-20"
          style={{
            backgroundImage: `radial-gradient(circle at 50% 50%, #444, #000)`,
            filter: 'contrast(120%) brightness(80%)'
          }}
        />
        <div className="absolute inset-0 bg-gradient-to-t from-[#0a0a0a] via-transparent to-transparent" />
      </div>

      {/* Main Content Area */}
      <main
        id="dialogue-scroll-container"
        ref={scrollContainerRef}
        className="relative w-full max-w-2xl h-full px-8 py-24 overflow-y-auto scroll-smooth no-scrollbar"
        style={{ scrollbarWidth: 'none' }}
      >
        <div className="flex flex-col min-h-full">
          <div className="mb-16 opacity-30 text-[12px] uppercase tracking-[0.2em] font-sans">
            [ Dialogue State: {currentStepId.replace('_', ' ')} ]
          </div>

          {/* Message History */}
          <div className="flex-1">
            {history.map((msg) => (
              <DialogueMessage key={msg.id} message={msg} />
            ))}

            {/* Streaming Array */}
            {streamingMessages.map((msg, idx) => (
              <div key={`stream-${msg.id}-${idx}`} className="mb-6 opacity-80">
                <DialogueMessage message={msg} isStreaming={idx === streamingMessages.length - 1} />
              </div>
            ))}

            {/* Streaming text */}
            <AnimatePresence>
              {streamingText !== null && streamingText.trim().length > 0 && streamingMessages.length === 0 && (
                <motion.div
                  initial={{ opacity: 0, y: 10 }}
                  animate={{ opacity: 1, y: 0 }}
                  className="mb-6"
                >
                  <DialogueMessage
                    message={{
                      id: streamingId ?? 'streaming',
                      speaker: 'SYSTEM',
                      type: 'SYSTEM',
                      text: streamingText,
                    }}
                    isStreaming
                  />
                </motion.div>
              )}
            </AnimatePresence>

            <AnimatePresence>
              {currentCheck && (
                <DiceRoller
                  {...currentCheck}
                  onComplete={handleRollComplete}
                />
              )}
            </AnimatePresence>
            {isTyping && streamingMessages.length === 0 && (streamingText === null || streamingText.trim().length === 0) && <TypingIndicator />}
            <div ref={messagesEndRef} className="h-4" />
          </div>

          {/* Current Options */}
          <AnimatePresence mode="wait">
            {!isTyping && !currentCheck && (dynamicOptions || currentStep?.options) && (
              <DialogueOptions
                key={dynamicOptions ? 'dynamic' : currentStepId}
                options={dynamicOptions || currentStep?.options || []}
                onSelect={handleOptionSelect}
              />
            )}
          </AnimatePresence>

          <div className="h-32" />
        </div>
      </main>

      <DebugPanel />
      <div className="fixed left-0 top-0 bottom-0 w-2 bg-gradient-to-r from-black/50 to-transparent" />
      <div className="fixed right-0 top-0 bottom-0 w-2 bg-gradient-to-l from-black/50 to-transparent" />
    </div>
  );
}
