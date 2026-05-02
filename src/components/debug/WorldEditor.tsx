import React, { useState, useEffect, useRef } from "react";
import { AnimatePresence, motion } from "motion/react";
import { Database, Save, Plus, X, AlertCircle, ChevronDown, ChevronRight } from "lucide-react";
import { WorldState, WorldEntity, Character } from "@/types/entities";
import { ResizableTextarea } from "./shared";

const TYPE_META = {
  CHARACTER: { color: "#c678dd", dimColor: "rgba(198,120,221,0.12)", label: "CHARACTER" },
  LOCATION: { color: "#61afef", dimColor: "rgba(97,175,239,0.12)", label: "LOCATION" },
  OBJECT: { color: "#d19a66", dimColor: "rgba(209,154,102,0.12)", label: "OBJECT" },
} as const;

function parseAttrValue(v: string): string | number | boolean {
  if (v === "true") return true;
  if (v === "false") return false;
  const n = Number(v);
  if (v !== "" && !isNaN(n)) return n;
  return v;
}

const FieldLabel: React.FC<{ label: string }> = ({ label }) => (
  <span className="text-[9px] font-bold uppercase tracking-[0.2em] text-white/25 block mb-1.5">
    {label}
  </span>
);

const StatBar: React.FC<{
  label: string;
  value: number;
  onChange: (v: number) => void;
}> = ({ label, value, onChange }) => {
  const max = Math.max(10, value);
  const segments = 10;
  const filled = Math.round((value / max) * segments);
  return (
    <div className="flex items-center gap-3 py-0.5 group/stat">
      <span className="text-[10px] text-white/35 font-mono w-28 flex-shrink-0 truncate uppercase tracking-wider">
        {label}
      </span>
      <div className="flex gap-px flex-1">
        {Array.from({ length: segments }).map((_, i) => (
          <button
            key={i}
            onClick={() => onChange(Math.max(1, Math.round(((i + 1) / segments) * max)))}
            className={`h-2 flex-1 transition-colors ${
              i < filled
                ? "bg-[#ff6b35]/55 hover:bg-[#ff6b35]/80"
                : "bg-white/8 hover:bg-white/20"
            }`}
          />
        ))}
      </div>
      <span className="text-[#d19a66] text-[11px] font-mono w-4 text-right tabular-nums">
        {value}
      </span>
      <div className="flex gap-px opacity-0 group-hover/stat:opacity-100 transition-opacity">
        <button
          onClick={() => onChange(Math.max(0, value - 1))}
          className="w-5 h-5 flex items-center justify-center bg-white/5 hover:bg-white/15 text-white/40 hover:text-white text-[10px] rounded-sm transition-colors"
        >
          −
        </button>
        <button
          onClick={() => onChange(value + 1)}
          className="w-5 h-5 flex items-center justify-center bg-white/5 hover:bg-white/15 text-white/40 hover:text-white text-[10px] rounded-sm transition-colors"
        >
          +
        </button>
      </div>
    </div>
  );
};

const AttributeTable: React.FC<{
  attrs: Record<string, string | number | boolean>;
  onChange: (attrs: Record<string, string | number | boolean>) => void;
}> = ({ attrs, onChange }) => {
  const entries = Object.entries(attrs);
  const update = (oldKey: string, newKey: string, val: string) => {
    const next: Record<string, string | number | boolean> = {};
    for (const [k, v] of entries) {
      if (k === oldKey) next[newKey] = parseAttrValue(val);
      else next[k] = v;
    }
    onChange(next);
  };
  const remove = (key: string) => {
    const next = { ...attrs };
    delete next[key];
    onChange(next);
  };
  const add = () => {
    let key = "new_key";
    let i = 1;
    while (key in attrs) key = `new_key_${i++}`;
    onChange({ ...attrs, [key]: "" });
  };

  return (
    <div className="space-y-px">
      {entries.map(([k, v]) => (
        <div key={k} className="flex items-center gap-px group/row">
          <input
            className="flex-1 bg-white/[0.03] border border-white/8 px-2 py-1 text-[11px] font-mono text-[#e06c75] focus:outline-none focus:border-white/20 transition-colors rounded-sm"
            defaultValue={k}
            onBlur={(e) => e.target.value !== k && update(k, e.target.value, String(v))}
          />
          <input
            className="flex-1 bg-white/[0.03] border border-white/8 px-2 py-1 text-[11px] font-mono text-[#98c379] focus:outline-none focus:border-white/20 transition-colors rounded-sm"
            defaultValue={String(v)}
            onBlur={(e) => update(k, k, e.target.value)}
          />
          <button
            onClick={() => remove(k)}
            className="w-7 h-7 flex items-center justify-center text-white/20 hover:text-[#e06c75] opacity-0 group-hover/row:opacity-100 transition-all flex-shrink-0"
          >
            <X size={10} />
          </button>
        </div>
      ))}
      <button
        onClick={add}
        className="w-full flex items-center gap-2 px-2 py-1.5 text-[9px] uppercase tracking-widest font-bold text-white/20 hover:text-white/50 border border-dashed border-white/8 hover:border-white/20 transition-all rounded-sm"
      >
        <Plus size={10} />
        Add entry
      </button>
    </div>
  );
};

const OpinionPills: React.FC<{
  opinions: Record<string, string>;
  onChange: (opinions: Record<string, string>) => void;
}> = ({ opinions, onChange }) => {
  const [editingKey, setEditingKey] = useState<string | null>(null);
  const [editVal, setEditVal] = useState("");
  const entries = Object.entries(opinions);

  const startEdit = (k: string) => {
    setEditingKey(k);
    setEditVal(opinions[k]);
  };
  const applyEdit = () => {
    if (!editingKey) return;
    onChange({ ...opinions, [editingKey]: editVal });
    setEditingKey(null);
  };
  const remove = (k: string) => {
    const next = { ...opinions };
    delete next[k];
    onChange(next);
    if (editingKey === k) setEditingKey(null);
  };
  const add = () => {
    let key = "new_entity";
    let i = 1;
    while (key in opinions) key = `new_entity_${i++}`;
    onChange({ ...opinions, [key]: "" });
    startEdit(key);
  };

  return (
    <div className="space-y-2">
      <div className="flex flex-wrap gap-1.5">
        {entries.map(([k, v]) => (
          <button
            key={k}
            onClick={() => startEdit(k)}
            className={`flex items-center gap-1.5 px-2 py-1 rounded-sm text-[10px] font-mono border transition-all ${
              editingKey === k
                ? "bg-[#c678dd]/15 border-[#c678dd]/40 text-[#c678dd]"
                : "bg-[#c678dd]/8 border-[#c678dd]/15 text-[#c678dd]/70 hover:border-[#c678dd]/30"
            }`}
          >
            <span className="truncate max-w-[80px]">{k}</span>
            <span
              className="text-white/20 hover:text-[#e06c75] ml-1"
              onClick={(e) => {
                e.stopPropagation();
                remove(k);
              }}
            >
              <X size={8} />
            </span>
          </button>
        ))}
        <button
          onClick={add}
          className="flex items-center gap-1 px-2 py-1 rounded-sm text-[9px] font-bold uppercase tracking-wider border border-dashed border-white/10 text-white/20 hover:text-white/40 hover:border-white/25 transition-all"
        >
          <Plus size={9} />
          Add
        </button>
      </div>
      <AnimatePresence>
        {editingKey && (
          <motion.div
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: "auto", opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            className="overflow-hidden"
          >
            <div className="p-3 bg-white/[0.03] border border-white/8 rounded-sm space-y-2">
              <div className="text-[9px] font-bold text-[#c678dd]/60 uppercase tracking-widest">
                {editingKey}
              </div>
              <ResizableTextarea
                autoFocus
                className="w-full bg-transparent text-[11px] font-mono text-white/70 focus:outline-none leading-relaxed"
                initialHeight={36}
                minHeight={28}
                value={editVal}
                onChange={(e) => setEditVal(e.target.value)}
              />
              <div className="flex gap-2">
                <button
                  onClick={applyEdit}
                  className="px-3 py-1 bg-[#c678dd]/10 border border-[#c678dd]/20 text-[#c678dd] text-[9px] font-bold uppercase tracking-wider rounded-sm hover:bg-[#c678dd]/20 transition-colors"
                >
                  Apply
                </button>
                <button
                  onClick={() => setEditingKey(null)}
                  className="px-3 py-1 text-white/20 text-[9px] font-bold uppercase tracking-wider hover:text-white/40 transition-colors"
                >
                  Cancel
                </button>
              </div>
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
};

export const WorldEditor: React.FC = () => {
  const [world, setWorld] = useState<WorldState | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [editing, setEditing] = useState<WorldEntity | null>(null);
  const [isSaving, setIsSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [savedFlash, setSavedFlash] = useState(false);
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});

  const fetchWorld = async () => {
    try {
      const res = await fetch("/api/world");
      if (res.ok) setWorld(await res.json());
    } catch {
      setError("Failed to load world state");
    }
  };

  useEffect(() => {
    fetchWorld();
  }, []);

  const selectEntity = (entity: WorldEntity) => {
    setSelectedId(entity.id);
    setEditing(JSON.parse(JSON.stringify(entity)));
    setError(null);
  };

  const updateField = (field: string, value: unknown) => {
    if (!editing) return;
    setEditing({ ...editing, [field]: value } as WorldEntity);
  };

  const updateStats = (key: string, value: number) => {
    if (!editing || editing.type !== "CHARACTER") return;
    const ch = editing as Character;
    setEditing({ ...ch, stats: { ...ch.stats, [key]: value } });
  };

  const handleSave = async () => {
    if (!editing) return;
    setIsSaving(true);
    setError(null);
    try {
      const res = await fetch("/api/world/entity", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(editing),
      });
      if (res.ok) {
        await fetchWorld();
        setSavedFlash(true);
        setTimeout(() => setSavedFlash(false), 1500);
      } else {
        throw new Error(await res.text());
      }
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setIsSaving(false);
    }
  };

  const addNewEntity = () => {
    const blank: WorldEntity = {
      id: `entity_${Date.now()}`,
      type: "OBJECT",
      displayName: "New Entity",
      shortDescription: "",
      longDescription: "",
      attributes: {},
    };
    selectEntity(blank);
  };

  const allEntities = world
    ? [
        ...Object.values(world.characters),
        ...Object.values(world.locations),
        ...Object.values(world.objects),
      ]
    : [];

  const grouped = {
    CHARACTER: allEntities.filter((e) => e.type === "CHARACTER"),
    LOCATION: allEntities.filter((e) => e.type === "LOCATION"),
    OBJECT: allEntities.filter((e) => e.type === "OBJECT"),
  } as const;

  const inputCls =
    "w-full bg-white/[0.03] border border-white/8 rounded-sm px-3 py-1.5 text-[11px] font-mono text-white/80 focus:outline-none focus:border-white/25 transition-colors placeholder:text-white/15";

  return (
    <div className="flex h-full overflow-hidden gap-0">
      {/* Sidebar */}
      <div className="w-64 flex-shrink-0 flex flex-col border-r border-white/8 pr-4 mr-4">
        <div className="flex items-center gap-2 h-9 mb-4 text-white/50">
          <Database size={14} />
          <span className="text-[10px] font-bold uppercase tracking-[0.2em]">Entity_Manifest</span>
        </div>
        <div className="flex-1 overflow-y-auto debug-scrollbar space-y-3 pr-1">
          {(["CHARACTER", "LOCATION", "OBJECT"] as const).map((type) => {
            const entities = grouped[type];
            if (entities.length === 0) return null;
            const meta = TYPE_META[type];
            const isCollapsed = collapsed[type];
            return (
              <div key={type}>
                <button
                  onClick={() => setCollapsed((p) => ({ ...p, [type]: !p[type] }))}
                  className="flex items-center gap-2 w-full mb-1.5 group"
                >
                  <div className="w-3 h-3 flex items-center justify-center text-white/20 group-hover:text-white/40 transition-colors">
                    {isCollapsed ? <ChevronRight size={10} /> : <ChevronDown size={10} />}
                  </div>
                  <span
                    className="text-[9px] font-bold uppercase tracking-[0.2em]"
                    style={{ color: meta.color }}
                  >
                    {type}
                  </span>
                  <span className="text-[9px] text-white/20 font-mono ml-auto">
                    {entities.length}
                  </span>
                </button>
                <AnimatePresence>
                  {!isCollapsed && (
                    <motion.div
                      initial={{ height: 0, opacity: 0 }}
                      animate={{ height: "auto", opacity: 1 }}
                      exit={{ height: 0, opacity: 0 }}
                      className="overflow-hidden space-y-0.5"
                    >
                      {entities.map((entity) => (
                        <button
                          key={entity.id}
                          onClick={() => selectEntity(entity)}
                          className={`w-full text-left px-2.5 py-2 rounded-sm text-[11px] transition-all flex items-center gap-2 ${
                            selectedId === entity.id
                              ? "bg-white/8 text-white"
                              : "text-white/40 hover:bg-white/[0.03] hover:text-white/60"
                          }`}
                          style={
                            selectedId === entity.id
                              ? { borderLeft: `2px solid ${meta.color}`, paddingLeft: "8px" }
                              : { borderLeft: "2px solid transparent" }
                          }
                        >
                          <span className="truncate flex-1 font-mono">{entity.displayName}</span>
                        </button>
                      ))}
                    </motion.div>
                  )}
                </AnimatePresence>
              </div>
            );
          })}
          <button
            onClick={addNewEntity}
            className="w-full flex items-center gap-2 px-2.5 py-2 text-[9px] uppercase tracking-widest font-bold text-white/20 hover:text-white/40 border border-dashed border-white/8 hover:border-white/20 transition-all rounded-sm"
          >
            <Plus size={10} />
            Append_Entity
          </button>
        </div>
      </div>

      {/* Form */}
      <div className="flex-1 flex flex-col min-w-0 overflow-hidden">
        {editing ? (
          <>
            <div className="flex items-center justify-between h-9 mb-5 flex-shrink-0">
              <div className="flex items-center gap-3">
                <span
                  className="text-[9px] font-bold uppercase tracking-wider px-2 py-0.5 rounded-sm border"
                  style={{
                    color: TYPE_META[editing.type].color,
                    borderColor: `${TYPE_META[editing.type].color}33`,
                    background: TYPE_META[editing.type].dimColor,
                  }}
                >
                  {editing.type}
                </span>
                <span className="text-[10px] font-mono text-white/25 truncate max-w-[240px]">
                  {editing.id}
                </span>
              </div>
              <button
                onClick={handleSave}
                disabled={isSaving}
                className={`flex items-center gap-2 px-3 py-1 rounded-sm border text-[10px] font-bold uppercase tracking-wider transition-all disabled:opacity-50 ${
                  savedFlash
                    ? "bg-[#98c379]/15 text-[#98c379] border-[#98c379]/30"
                    : "bg-white/5 text-white/60 hover:bg-white/10 hover:text-white border-white/10"
                }`}
              >
                <Save size={12} />
                {isSaving ? "Syncing..." : savedFlash ? "Saved ✓" : "Update_Manifest"}
              </button>
            </div>

            {error && (
              <div className="mb-4 p-3 bg-red-500/10 border border-red-500/20 rounded-sm flex items-center gap-2 text-red-400 text-[11px] flex-shrink-0">
                <AlertCircle size={12} />
                <span>{error}</span>
              </div>
            )}

            <div className="flex-1 overflow-y-auto debug-scrollbar space-y-5 pr-2">
              {/* Core fields */}
              <div>
                <FieldLabel label="displayName" />
                <input
                  className={inputCls}
                  value={editing.displayName}
                  onChange={(e) => updateField("displayName", e.target.value)}
                />
              </div>
              <div>
                <FieldLabel label="shortDescription" />
                <input
                  className={inputCls}
                  value={editing.shortDescription}
                  onChange={(e) => updateField("shortDescription", e.target.value)}
                />
              </div>
              <div>
                <FieldLabel label="longDescription" />
                <ResizableTextarea
                  className={`${inputCls} leading-relaxed`}
                  initialHeight={66}
                  value={editing.longDescription}
                  onChange={(e) => updateField("longDescription", e.target.value)}
                />
              </div>

              {/* CHARACTER-only */}
              {editing.type === "CHARACTER" && (() => {
                const ch = editing as Character;
                const statsEntries = Object.entries(ch.stats ?? {});
                return (
                  <>
                    {statsEntries.length > 0 && (
                      <div>
                        <FieldLabel label={`Stats — ${statsEntries.length} values`} />
                        <div className="space-y-0.5 p-3 bg-white/[0.02] border border-white/6 rounded-sm">
                          {statsEntries.map(([key, val]) => (
                            <StatBar
                              key={key}
                              label={key.replace(/_/g, " ").toUpperCase()}
                              value={val as number}
                              onChange={(v) => updateStats(key, v)}
                            />
                          ))}
                        </div>
                      </div>
                    )}
                    <div>
                      <FieldLabel label={`Opinions — ${Object.keys(ch.opinions ?? {}).length} relations`} />
                      <OpinionPills
                        opinions={ch.opinions ?? {}}
                        onChange={(o) => updateField("opinions", o)}
                      />
                    </div>
                  </>
                );
              })()}

              {/* Attributes */}
              <div>
                <FieldLabel
                  label={`Attributes — ${Object.keys(editing.attributes ?? {}).length} entries`}
                />
                <AttributeTable
                  attrs={editing.attributes ?? {}}
                  onChange={(a) => updateField("attributes", a)}
                />
              </div>
            </div>
          </>
        ) : (
          <div className="flex-1 flex flex-col items-center justify-center text-white/5 select-none">
            <Database size={56} className="mb-5 opacity-40" />
            <p className="uppercase tracking-[0.35em] text-[9px] font-bold">
              Select_Manifest_Entry
            </p>
          </div>
        )}
      </div>
    </div>
  );
};
