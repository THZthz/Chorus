import React, { useState, useEffect, useCallback } from "react";
import { Save, RotateCcw, Check, AlertTriangle, Braces } from "lucide-react";

const TEMPLATE_VARS = [
  {
    var: "{{entities_brief}}",
    label: "Entity summaries grouped by type (CHARACTER, LOCATION, OBJECT) with id, displayName, and shortDescription.",
  },
  {
    var: "{{active_plots}}",
    label: "Full active plot tree rendering with status tags, involved entities, and childPlots branch options.",
  },
];

type Status = { type: "success" | "error"; message: string } | null;

export const SystemPromptEditor: React.FC = () => {
  const [template, setTemplate] = useState("");
  const [saved, setSaved] = useState(true);
  const [loading, setLoading] = useState(true);
  const [status, setStatus] = useState<Status>(null);

  useEffect(() => {
    fetch("/api/debug/system-prompt")
      .then((r) => r.json())
      .then((data) => {
        setTemplate(data.template);
        setLoading(false);
      })
      .catch(() => setLoading(false));
  }, []);

  const save = useCallback(async () => {
    setStatus(null);
    try {
      const res = await fetch("/api/debug/system-prompt", {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ template }),
      });
      if (!res.ok) {
        const err = await res.json();
        setStatus({ type: "error", message: err.error || "Save failed" });
        return;
      }
      setSaved(true);
      setStatus({ type: "success", message: "Prompt saved" });
      setTimeout(() => setStatus(null), 2000);
    } catch {
      setStatus({ type: "error", message: "Network error" });
    }
  }, [template]);

  const reset = useCallback(async () => {
    setStatus(null);
    try {
      const res = await fetch("/api/debug/system-prompt/reset", { method: "POST" });
      if (!res.ok) {
        setStatus({ type: "error", message: "Reset failed" });
        return;
      }
      const reload = await fetch("/api/debug/system-prompt");
      const data = await reload.json();
      setTemplate(data.template);
      setSaved(true);
      setStatus({ type: "success", message: "Reset to default" });
      setTimeout(() => setStatus(null), 2000);
    } catch {
      setStatus({ type: "error", message: "Network error" });
    }
  }, []);

  const onChange = useCallback((e: React.ChangeEvent<HTMLTextAreaElement>) => {
    setTemplate(e.target.value);
    setSaved(false);
    setStatus(null);
  }, []);

  if (loading) {
    return (
      <div className="flex items-center justify-center h-full text-white/30 text-xs">Loading...</div>
    );
  }

  return (
    <div className="flex flex-col h-full gap-3">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <h2 className="text-xs font-bold uppercase tracking-[0.2em] text-white/60">
            GM System Prompt
          </h2>
          {!saved && (
            <span className="text-[10px] text-amber-400/80 italic">Unsaved changes</span>
          )}
          {status && (
            <span
              className={`text-[10px] flex items-center gap-1 ${
                status.type === "success" ? "text-emerald-400" : "text-red-400"
              }`}
            >
              {status.type === "success" ? <Check size={10} /> : <AlertTriangle size={10} />}
              {status.message}
            </span>
          )}
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={reset}
            className="px-3 py-1.5 flex items-center gap-1.5 text-[10px] font-bold uppercase tracking-wider text-white/40 hover:text-white/80 border border-white/10 hover:border-white/20 rounded-sm transition-colors"
            title="Reset to default system prompt"
          >
            <RotateCcw size={10} />
            Reset
          </button>
          <button
            onClick={save}
            disabled={saved}
            className="px-3 py-1.5 flex items-center gap-1.5 text-[10px] font-bold uppercase tracking-wider bg-emerald-500/20 border border-emerald-500/40 text-emerald-300 hover:bg-emerald-500/30 disabled:opacity-30 disabled:cursor-not-allowed rounded-sm transition-colors"
          >
            <Save size={10} />
            Save
          </button>
        </div>
      </div>

      <div className="flex-1 min-h-0 flex gap-3">
        <textarea
          value={template}
          onChange={onChange}
          spellCheck={false}
          className="flex-1 bg-[#0d0d0f] border border-white/10 rounded-sm p-4 text-xs font-mono text-white/80 resize-none focus:outline-none focus:border-white/20 placeholder-white/20"
          placeholder="Enter system prompt template..."
        />

        <div className="w-56 shrink-0 flex flex-col gap-3">
          <div className="bg-[#0d0d0f] border border-white/10 rounded-sm p-3">
            <div className="flex items-center gap-1.5 mb-2">
              <Braces size={10} className="text-amber-400/70" />
              <span className="text-[10px] font-bold uppercase tracking-wider text-white/40">
                Template Variables
              </span>
            </div>
            <p className="text-[10px] text-white/25 mb-3 leading-relaxed">
              These placeholders are replaced with live data at generation time.
            </p>
            <div className="flex flex-col gap-2.5">
              {TEMPLATE_VARS.map((v) => (
                <div key={v.var}>
                  <code className="text-[11px] text-amber-400/80 font-mono bg-amber-400/5 px-1 py-0.5 rounded-sm">
                    {v.var}
                  </code>
                  <p className="text-[10px] text-white/35 mt-0.5 leading-relaxed">{v.label}</p>
                </div>
              ))}
            </div>
          </div>

          <div className="bg-[#0d0d0f] border border-white/10 rounded-sm p-3">
            <span className="text-[10px] font-bold uppercase tracking-wider text-white/40">
              Usage
            </span>
            <p className="text-[10px] text-white/30 mt-1.5 leading-relaxed">
              The template is the full system prompt. Only{" "}
              <code className="text-white/50 text-[10px]">{"{{variables}}"}</code> are dynamic.
              Everything else is sent verbatim to the LLM.
            </p>
          </div>
        </div>
      </div>
    </div>
  );
};
