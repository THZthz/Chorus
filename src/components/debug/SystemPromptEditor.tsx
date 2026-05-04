import React, { useState, useEffect, useCallback, useMemo } from "react";
import { Save, RotateCcw, FileText, Check, AlertTriangle, Braces } from "lucide-react";
import CodeMirror from "@uiw/react-codemirror";
import { EditorView, ViewPlugin, Decoration, DecorationSet, ViewUpdate } from "@codemirror/view";
import { HighlightStyle, syntaxHighlighting, syntaxTree } from "@codemirror/language";
import { tags as t } from "@lezer/highlight";
import { jsonLanguage } from "@codemirror/lang-json";
import { markdownLanguage } from "@codemirror/lang-markdown";
import markdoc from "@markdoc/markdoc";
import type { Config } from "@markdoc/markdoc";
import richEditor from "codemirror-rich-markdoc";
import { createTableExtension, tableDarkTheme } from "@markwhen/codemirror-tables";

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

markdoc.transformer.findSchema = (node, config) => {
  return node.tag
    ? config?.tags?.[node.tag] ?? config?.tags?.$$fallback
    : config?.nodes?.[node.type];
};

const markdocConfig: Config = {
  tags: {
    $$fallback: {
      transform(node, config) {
        const children = node.transformChildren(config);
        return new markdoc.Tag("div", { class: "cm-markdoc-fallbackTag" }, [
          new markdoc.Tag("div", { class: "cm-markdoc-fallbackTag--name" }, [node?.tag ?? ""]),
          new markdoc.Tag("div", { class: "cm-markdoc-fallbackTag--inner" }, children),
        ]);
      },
    },
  },
};

const textColor = "rgba(255,255,255,0.78)";
const mutedColor = "rgba(255,255,255,0.35)";
const accentColor = "rgba(255,200,130,0.7)";

const debugHighlightStyle = HighlightStyle.define([
  { tag: t.heading1, fontWeight: "bold", fontSize: "15px", color: textColor, fontFamily: "inherit" },
  { tag: t.heading2, fontWeight: "bold", fontSize: "13px", color: textColor, fontFamily: "inherit" },
  { tag: t.heading3, fontWeight: "bold", fontSize: "12px", color: textColor, fontFamily: "inherit" },
  { tag: t.heading4, fontWeight: "bold", fontSize: "11px", color: textColor, fontFamily: "inherit" },
  { tag: t.link, textDecoration: "underline", color: "rgba(100,170,255,0.55)", fontFamily: "inherit" },
  { tag: t.emphasis, fontStyle: "italic", fontFamily: "inherit" },
  { tag: t.strong, fontWeight: "bold", fontFamily: "inherit" },
  { tag: t.monospace, fontFamily: "'JetBrains Mono','Fira Code',monospace" },
  { tag: t.content, color: textColor, fontFamily: "inherit" },
  { tag: t.meta, color: mutedColor, fontFamily: "inherit" },
  { tag: t.strikethrough, textDecoration: "line-through", color: mutedColor, fontFamily: "inherit" },
  { tag: t.url, color: "rgba(100,170,255,0.45)", fontFamily: "inherit" },
  { tag: t.processingInstruction, color: accentColor, fontFamily: "inherit" },
]);

const debugEditorTheme = EditorView.theme({
  "&": {
    backgroundColor: "#0d0d0f",
  },
  ".cm-content": {
    caretColor: "rgba(255,255,255,0.5)",
    fontFamily: "'Inter','Helvetica Neue',sans-serif",
    fontSize: "12px",
    lineHeight: "1.7",
    padding: "14px 16px",
  },
  ".cm-scroller": {
    fontFamily: "'Inter','Helvetica Neue',sans-serif",
    lineHeight: "1.7",
  },
  "&.cm-focused .cm-cursor": {
    borderLeftColor: "rgba(255,255,255,0.5)",
  },
  "&.cm-focused": {
    outline: "none",
  },
  ".cm-activeLine": {
    backgroundColor: "transparent",
  },
  ".cm-selectionMatch": {
    backgroundColor: "transparent",
  },
  ".cm-selectionBackground": {
    backgroundColor: "rgba(255,255,255,0.07)",
  },
  ".cm-placeholder": {
    color: "rgba(255,255,255,0.15)",
  },
}, { dark: true });

const tableOverrides = EditorView.theme({
  "&": {
    "--table-toolbar-bg": "#2d2d2d",
    "--table-toolbar-color": "#e0e0e0",
    "--table-toolbar-hover-bg": "#3d3d3d",
    "--table-toolbar-border": "#444",
  },
}, { dark: true });

// JSON fenced-code-block highlight colors — matches JsonExplorer/JsonNode theme
const jsonHighlightTheme = EditorView.theme({
  ".cm-json-string": { color: "#98c379" },
  ".cm-json-number": { color: "#d19a66" },
  ".cm-json-bool": { color: "#c678dd", fontWeight: "bold" },
  ".cm-json-null": { color: "#5c6370" },
  ".cm-json-property": { color: "#e06c75" },
  ".cm-json-separator": { color: "#abb2bf" },
  ".cm-json-bracket": { color: "#abb2bf" },
}, { dark: true });

const jsonParser = jsonLanguage.parser;

const jsonNodeMarks: Record<string, Decoration> = {
  String: Decoration.mark({ class: "cm-json-string" }),
  Number: Decoration.mark({ class: "cm-json-number" }),
  True: Decoration.mark({ class: "cm-json-bool" }),
  False: Decoration.mark({ class: "cm-json-bool" }),
  Null: Decoration.mark({ class: "cm-json-null" }),
  PropertyName: Decoration.mark({ class: "cm-json-property" }),
  ",": Decoration.mark({ class: "cm-json-separator" }),
  ":": Decoration.mark({ class: "cm-json-separator" }),
  "[": Decoration.mark({ class: "cm-json-bracket" }),
  "]": Decoration.mark({ class: "cm-json-bracket" }),
  "{": Decoration.mark({ class: "cm-json-bracket" }),
  "}": Decoration.mark({ class: "cm-json-bracket" }),
};

function jsonCodeBlockHighlight() {
  return ViewPlugin.fromClass(
    class {
      decorations: DecorationSet;
      constructor(view: EditorView) {
        this.decorations = this.compute(view);
      }
      update(update: ViewUpdate) {
        if (update.docChanged || update.viewportChanged) {
          this.decorations = this.compute(update.view);
        }
      }
      compute(view: EditorView): DecorationSet {
        const widgets: { from: number; to: number; value: Decoration }[] = [];
        for (const { from, to } of view.visibleRanges) {
          syntaxTree(view.state).iterate({
            from,
            to,
            enter(node) {
              if (node.name !== "FencedCode") return;
              let codeTextFrom = -1;
              let codeTextTo = -1;
              const cursor = node.node.cursor();
              if (!cursor.firstChild()) return;
              do {
                if (cursor.type.name === "CodeInfo") {
                  const raw = view.state.doc.sliceString(cursor.from, cursor.to);
                  if (!/^(jsonc?)$/i.test(raw.trim())) return;
                }
                if (cursor.type.name === "CodeText") {
                  codeTextFrom = cursor.from;
                  codeTextTo = cursor.to;
                }
              } while (cursor.nextSibling());
              if (codeTextFrom < 0) return;
              const code = view.state.doc.sliceString(codeTextFrom, codeTextTo);
              let jsonTree;
              try {
                jsonTree = jsonParser.parse(code);
              } catch {
                return;
              }
              const jc = jsonTree.cursor();
              do {
                const mark = jsonNodeMarks[jc.type.name];
                if (mark && jc.from < jc.to) {
                  widgets.push({
                    from: codeTextFrom + jc.from,
                    to: codeTextFrom + jc.to,
                    value: mark,
                  });
                }
              } while (jc.next());
            },
          });
        }
        return Decoration.set(widgets, true);
      }
    },
    { decorations: (v) => v.decorations },
  );
}

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

  const loadDefault = useCallback(async () => {
    setStatus(null);
    try {
      const res = await fetch("/api/debug/system-prompt/default");
      const data = await res.json();
      setTemplate(data.template);
      setSaved(false);
      setStatus({ type: "success", message: "Default loaded" });
      setTimeout(() => setStatus(null), 2000);
    } catch {
      setStatus({ type: "error", message: "Network error" });
    }
  }, []);

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

  const extensions = useMemo(
    () => [
      richEditor({ markdoc: markdocConfig, lezer: { base: markdownLanguage } }),
      syntaxHighlighting(debugHighlightStyle),
      jsonCodeBlockHighlight(),
      jsonHighlightTheme,
      EditorView.lineWrapping,
      createTableExtension({
        cellEditorExtensions: [debugEditorTheme],
      }),
      tableDarkTheme,
      tableOverrides,
    ],
    [],
  );

  const handleChange = useCallback((value: string) => {
    setTemplate(value);
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
            onClick={loadDefault}
            className="px-3 py-1.5 flex items-center gap-1.5 text-[10px] font-bold uppercase tracking-wider text-white/40 hover:text-white/80 border border-white/10 hover:border-white/20 rounded-sm transition-colors"
            title="Load default system prompt into editor without saving"
          >
            <FileText size={10} />
            Load Default
          </button>
          <button
            onClick={reset}
            className="px-3 py-1.5 flex items-center gap-1.5 text-[10px] font-bold uppercase tracking-wider text-white/40 hover:text-white/80 border border-white/10 hover:border-white/20 rounded-sm transition-colors"
            title="Reset to default system prompt and save"
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
        <div className="flex-1 border border-white/10 rounded-sm overflow-auto debug-scrollbar">
          <CodeMirror
            value={template}
            onChange={handleChange}
            extensions={extensions}
            theme={debugEditorTheme}
            height="auto"
            basicSetup={{
              lineNumbers: false,
              foldGutter: false,
              highlightActiveLine: false,
              highlightActiveLineGutter: false,
              drawSelection: false,
              bracketMatching: false,
              closeBrackets: false,
              autocompletion: false,
              crosshairCursor: false,
              highlightSelectionMatches: false,
            }}
          />
        </div>

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
