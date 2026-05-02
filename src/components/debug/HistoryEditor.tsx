import React, { useState, useEffect, useRef } from "react";
import { AnimatePresence, motion } from "motion/react";
import {
  MessageSquare,
  Save,
  Plus,
  X,
  AlertCircle,
  GripVertical,
  Dice6,
  Bell,
  Cpu,
} from "lucide-react";
import { Message, SpeakerType } from "@/types/dialogue";
import { CustomSelect, ResizableTextarea } from "@/components/debug/shared";

const SPEAKER_TYPES: SpeakerType[] = [
  "YOU",
  "CHARACTER",
  "INNER_VOICE",
  "SYSTEM",
  "ROLL",
  "NOTIFICATION",
];

const NOTIFICATION_TYPES = ["XP", "TASK", "ITEM"] as const;

// Disco-skill colour map for INNER_VOICE
const SKILL_COLORS: Record<string, string> = {
  LOGIC: "#61afef",
  RHETORIC: "#e06c75",
  EMPATHY: "#c678dd",
  PERCEPTION: "#98c379",
  VOLITION: "#eab308",
  ENDURANCE: "#d19a66",
  "INLAND EMPIRE": "#c678dd",
  SUGGESTION: "#61afef",
  "HALF LIGHT": "#e06c75",
  "PHYSICAL INSTRUMENT": "#d19a66",
  INTERFACING: "#98c379",
  ELECTROCHEMISTRY: "#eab308",
};

function skillColor(speaker: string): string {
  return SKILL_COLORS[speaker.toUpperCase()] ?? "#abb2bf";
}

const YouCard: React.FC<{ msg: Message }> = ({ msg }) => (
  <div className="ml-auto max-w-[80%] px-4 py-2.5 bg-white/[0.05] border border-white/15 rounded-sm">
    <p className="text-[12px] text-white/80 leading-relaxed font-sans">{msg.text}</p>
  </div>
);

const CharacterCard: React.FC<{ msg: Message }> = ({ msg }) => (
  <div className="px-4 py-3 bg-[#61afef]/[0.04] border border-[#61afef]/15 rounded-sm">
    <div className="text-[9px] font-bold uppercase tracking-[0.2em] text-[#61afef]/70 mb-1.5">
      {msg.speaker}
    </div>
    <p className="text-[12px] text-white/75 leading-relaxed font-sans">{msg.text}</p>
  </div>
);

const InnerVoiceCard: React.FC<{ msg: Message }> = ({ msg }) => {
  const col = skillColor(msg.speaker);
  return (
    <div
      className="px-4 py-3 rounded-sm border"
      style={{ background: `${col}09`, borderColor: `${col}22` }}
    >
      <div
        className="text-[9px] font-bold uppercase tracking-[0.2em] mb-1.5 flex items-center gap-2"
        style={{ color: col }}
      >
        <Cpu size={9} />
        {msg.speaker}
      </div>
      <p className="text-[12px] leading-relaxed italic font-sans" style={{ color: `${col}cc` }}>
        {msg.text}
      </p>
    </div>
  );
};

const SystemCard: React.FC<{ msg: Message }> = ({ msg }) => (
  <div className="flex items-center gap-3 py-1">
    <div className="flex-1 h-px bg-white/8" />
    <span className="text-[10px] text-white/20 font-mono tracking-wider">{msg.text}</span>
    <div className="flex-1 h-px bg-white/8" />
  </div>
);

const RollCard: React.FC<{ msg: Message }> = ({ msg }) => (
  <div className="px-4 py-3 bg-[#eab308]/[0.04] border border-[#eab308]/15 rounded-sm flex items-center gap-4">
    <Dice6 size={14} className="text-[#eab308]/60 flex-shrink-0" />
    <span className="text-[11px] font-mono text-white/60 flex-1">{msg.text}</span>
  </div>
);

const NotificationCard: React.FC<{ msg: Message }> = ({ msg }) => {
  const nt = msg.metadata?.notificationType;
  const color = nt === "XP" ? "#98c379" : nt === "TASK" ? "#61afef" : "#d19a66";
  return (
    <div
      className="px-4 py-2.5 rounded-sm border flex items-center gap-3"
      style={{ background: `${color}08`, borderColor: `${color}20` }}
    >
      <Bell size={11} style={{ color }} className="flex-shrink-0" />
      {nt && (
        <span
          className="text-[9px] font-bold uppercase tracking-widest px-1.5 py-0.5 rounded-sm border"
          style={{ color, borderColor: `${color}40`, background: `${color}15` }}
        >
          {nt}
        </span>
      )}
      <span className="text-[11px] font-mono text-white/60">{msg.text}</span>
    </div>
  );
};

function MessagePreview({ msg }: { msg: Message }) {
  switch (msg.type) {
    case "YOU":
      return <YouCard msg={msg} />;
    case "CHARACTER":
      return <CharacterCard msg={msg} />;
    case "INNER_VOICE":
      return <InnerVoiceCard msg={msg} />;
    case "SYSTEM":
      return <SystemCard msg={msg} />;
    case "ROLL":
      return <RollCard msg={msg} />;
    case "NOTIFICATION":
      return <NotificationCard msg={msg} />;
  }
}

const EditOverlay: React.FC<{
  msg: Message;
  onApply: (updated: Message) => void;
  onClose: () => void;
}> = ({ msg, onApply, onClose }) => {
  const [type, setType] = useState<SpeakerType>(msg.type);
  const [speaker, setSpeaker] = useState(msg.speaker);
  const [text, setText] = useState(msg.text);
  const [notifType, setNotifType] = useState<string>(msg.metadata?.notificationType ?? "");
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose();
    };
    document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, [onClose]);

  const apply = () => {
    onApply({
      ...msg,
      type,
      speaker,
      text,
      metadata: notifType ? { notificationType: notifType as "XP" | "TASK" | "ITEM" } : undefined,
    });
    onClose();
  };

  return (
    <div
      ref={ref}
      className="absolute left-0 right-0 z-30 bg-[#111214] border border-white/15 rounded-sm shadow-2xl p-4 space-y-3"
      style={{ top: 0 }}
      onClick={(e) => e.stopPropagation()}
    >
      <div className="flex items-center justify-between mb-1">
        <span className="text-[9px] font-bold uppercase tracking-[0.2em] text-white/30">
          Edit Message
        </span>
        <button onClick={onClose} className="text-white/20 hover:text-white/60 transition-colors">
          <X size={12} />
        </button>
      </div>
      <div className="flex gap-3">
        <div className="flex-1">
          <label className="text-[9px] font-bold uppercase tracking-wider text-white/25 block mb-1">
            Type
          </label>
          <CustomSelect
            value={type}
            options={[...SPEAKER_TYPES]}
            onChange={(v) => setType(v as SpeakerType)}
          />
        </div>
        <div className="flex-1">
          <label className="text-[9px] font-bold uppercase tracking-wider text-white/25 block mb-1">
            Speaker
          </label>
          <input
            className="w-full bg-white/[0.04] border border-white/10 rounded-sm px-2 py-1 text-[11px] font-mono text-white/70 focus:outline-none focus:border-white/25"
            value={speaker}
            onChange={(e) => setSpeaker(e.target.value)}
          />
        </div>
        {type === "NOTIFICATION" && (
          <div className="flex-1">
            <label className="text-[9px] font-bold uppercase tracking-wider text-white/25 block mb-1">
              Badge
            </label>
            <CustomSelect
              value={notifType}
              options={[{ value: "", label: "None" }, ...NOTIFICATION_TYPES]}
              onChange={(v) => setNotifType(v)}
            />
          </div>
        )}
      </div>
      <div>
        <label className="text-[9px] font-bold uppercase tracking-wider text-white/25 block mb-1">
          Text
        </label>
        <ResizableTextarea
          autoFocus
          className="w-full bg-white/[0.04] border border-white/10 rounded-sm px-3 py-2 text-[11px] font-mono text-white/75 focus:outline-none focus:border-white/25 leading-relaxed"
          initialHeight={70}
          value={text}
          onChange={(e) => setText(e.target.value)}
        />
      </div>
      <div className="flex gap-2">
        <button
          onClick={apply}
          className="px-3 py-1 bg-[#98c379]/10 border border-[#98c379]/25 text-[#98c379] text-[9px] font-bold uppercase tracking-wider rounded-sm hover:bg-[#98c379]/20 transition-colors"
        >
          Apply
        </button>
        <button
          onClick={onClose}
          className="px-3 py-1 text-white/25 text-[9px] font-bold uppercase tracking-wider hover:text-white/50 transition-colors"
        >
          Cancel
        </button>
      </div>
    </div>
  );
};

export const HistoryEditor: React.FC = () => {
  const [messages, setMessages] = useState<Message[]>([]);
  const [isSaving, setIsSaving] = useState(false);
  const [savedFlash, setSavedFlash] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [dragIdx, setDragIdx] = useState<number | null>(null);
  const [dragOverIdx, setDragOverIdx] = useState<number | null>(null);

  const fetchHistory = async () => {
    try {
      const res = await fetch("/api/history");
      if (res.ok) setMessages(await res.json());
    } catch {
      setError("Failed to load history");
    }
  };

  useEffect(() => {
    fetchHistory();
  }, []);

  const handleSave = async () => {
    setIsSaving(true);
    setError(null);
    try {
      const res = await fetch("/api/history", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(messages),
      });
      if (!res.ok) throw new Error(await res.text());
      setSavedFlash(true);
      setTimeout(() => setSavedFlash(false), 1500);
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setIsSaving(false);
    }
  };

  const applyEdit = (updated: Message) => {
    setMessages((prev) => prev.map((m) => (m.id === updated.id ? updated : m)));
  };

  const removeMsg = (id: string) => {
    setMessages((prev) => prev.filter((m) => m.id !== id));
    if (editingId === id) setEditingId(null);
  };

  const addMessage = () => {
    const id = `msg_${Date.now()}`;
    const msg: Message = { id, speaker: "CHARACTER", type: "CHARACTER", text: "" };
    setMessages((prev) => [...prev, msg]);
    setEditingId(id);
  };

  const insertAfter = (afterId: string) => {
    const id = `msg_${Date.now()}`;
    const msg: Message = { id, speaker: "CHARACTER", type: "CHARACTER", text: "" };
    setMessages((prev) => {
      const idx = prev.findIndex((m) => m.id === afterId);
      const next = [...prev];
      next.splice(idx + 1, 0, msg);
      return next;
    });
    setEditingId(id);
  };

  // Drag reorder
  const handleDrop = (toIdx: number) => {
    if (dragIdx === null || dragIdx === toIdx) return;
    setMessages((prev) => {
      const next = [...prev];
      const [item] = next.splice(dragIdx, 1);
      next.splice(toIdx, 0, item);
      return next;
    });
    setDragIdx(null);
    setDragOverIdx(null);
  };

  return (
    <div className="flex flex-col h-full overflow-hidden">
      <div className="flex items-center justify-between h-9 mb-5 flex-shrink-0">
        <div className="flex items-center gap-2 text-white/50">
          <MessageSquare size={14} />
          <span className="text-[10px] font-bold uppercase tracking-[0.2em]">Dialogue_Buffer</span>
          <span className="text-white/20 text-[9px] font-mono ml-2">
            {messages.length} messages
          </span>
        </div>
        <button
          onClick={handleSave}
          disabled={isSaving}
          className={`flex items-center gap-2 px-3 py-1 rounded-sm border text-[10px] font-bold uppercase tracking-wider transition-all disabled:opacity-50 ${
            savedFlash
              ? "bg-[#98c379]/15 text-[#98c379] border-[#98c379]/30"
              : "bg-[#98c379]/8 text-[#98c379]/70 hover:bg-[#98c379]/15 border-[#98c379]/20"
          }`}
        >
          <Save size={12} />
          {isSaving ? "Syncing..." : savedFlash ? "Saved ✓" : "Sync_Buffer"}
        </button>
      </div>

      {error && (
        <div className="mb-4 p-3 bg-red-500/10 border border-red-500/20 rounded-sm flex items-center gap-2 text-red-400 text-[11px] flex-shrink-0">
          <AlertCircle size={12} />
          <span>{error}</span>
        </div>
      )}

      <div className="flex-1 overflow-y-auto debug-scrollbar space-y-2 pr-1">
        {messages.map((msg, idx) => (
          <div
            key={msg.id}
            className={`relative group/msg transition-opacity ${
              dragIdx === idx ? "opacity-30" : "opacity-100"
            } ${dragOverIdx === idx && dragIdx !== idx ? "border-t-2 border-[#ff6b35]/60" : ""}`}
            draggable
            onDragStart={() => setDragIdx(idx)}
            onDragOver={(e) => {
              e.preventDefault();
              setDragOverIdx(idx);
            }}
            onDrop={() => handleDrop(idx)}
            onDragEnd={() => {
              setDragIdx(null);
              setDragOverIdx(null);
            }}
          >
            {/* Drag handle */}
            <div className="absolute left-0 top-1/2 -translate-y-1/2 -translate-x-5 opacity-0 group-hover/msg:opacity-100 transition-opacity cursor-grab active:cursor-grabbing text-white/20">
              <GripVertical size={12} />
            </div>

            {/* Message card */}
            <div
              className="cursor-pointer"
              onClick={() => setEditingId(editingId === msg.id ? null : msg.id)}
            >
              <MessagePreview msg={msg} />
            </div>

            {/* Hover controls */}
            <div className="absolute top-1 right-1 flex gap-1 opacity-0 group-hover/msg:opacity-100 transition-opacity z-10">
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  insertAfter(msg.id);
                }}
                className="w-5 h-5 flex items-center justify-center bg-[#0a0a0a]/90 border border-white/10 text-white/30 hover:text-white/60 rounded-sm transition-colors"
                title="Insert after"
              >
                <Plus size={9} />
              </button>
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  removeMsg(msg.id);
                }}
                className="w-5 h-5 flex items-center justify-center bg-[#0a0a0a]/90 border border-white/10 text-white/30 hover:text-[#e06c75] rounded-sm transition-colors"
                title="Delete"
              >
                <X size={9} />
              </button>
            </div>

            {/* Inline edit overlay */}
            <AnimatePresence>
              {editingId === msg.id && (
                <motion.div
                  initial={{ opacity: 0, y: -4 }}
                  animate={{ opacity: 1, y: 0 }}
                  exit={{ opacity: 0, y: -4 }}
                  transition={{ duration: 0.12 }}
                  className="relative z-20 mt-1"
                >
                  <EditOverlay msg={msg} onApply={applyEdit} onClose={() => setEditingId(null)} />
                </motion.div>
              )}
            </AnimatePresence>
          </div>
        ))}

        {messages.length === 0 && (
          <div className="flex flex-col items-center justify-center py-16 text-white/10 select-none">
            <MessageSquare size={40} className="mb-4 opacity-40" />
            <p className="text-[9px] uppercase tracking-[0.35em] font-bold">Empty_Buffer</p>
          </div>
        )}

        {/* Add message */}
        <button
          onClick={addMessage}
          className="w-full flex items-center justify-center gap-2 py-3 text-[9px] uppercase tracking-widest font-bold text-white/20 hover:text-white/40 border border-dashed border-white/8 hover:border-white/20 transition-all rounded-sm mt-3"
        >
          <Plus size={10} />
          Add Message
        </button>
      </div>
    </div>
  );
};
