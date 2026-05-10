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

import React from "react";
import { TOOL_NAMES } from "@/shared/constants";
import { CopyButton } from "@/components/debug/CopyButton";
import { JsonExplorer } from "@/components/debug/JsonExplorer";
import { JsonNode } from "@/components/debug/JsonNode";

function readableToolName(name: string) {
  return name.replace(/([a-z])([A-Z])/g, "$1 $2");
}

export function isToolError(output: unknown): boolean {
  if (typeof output !== "string") return false;
  return /^(ERROR|VALIDATION FAILED)/.test(output);
}

// ── Lightweight markdown renderer ──

/** Render inline markdown spans: **bold**, `code` */
function InlineMarkdown({ text }: { text: string }) {
  const parts = text.split(/(\*\*.*?\*\*|`.*?`)/g);
  return (
    <>
      {parts.map((part, i) => {
        if (part.startsWith("**") && part.endsWith("**")) {
          return (
            <strong key={i} className="text-white/90 font-semibold">
              {part.slice(2, -2)}
            </strong>
          );
        }
        if (part.startsWith("`") && part.endsWith("`")) {
          return (
            <code
              key={i}
              className="text-[#eab308] bg-white/[0.04] px-1 py-[1px] rounded-sm font-mono text-[0.95em]"
            >
              {part.slice(1, -1)}
            </code>
          );
        }
        return <span key={i}>{part}</span>;
      })}
    </>
  );
}

const TEXT_CLASS =
  "text-[11px] text-white/40 whitespace-pre-wrap break-words leading-relaxed";
const TEXT_COMPACT = "text-[10px] text-white/40 whitespace-pre-wrap break-words leading-relaxed";

function MarkdownRenderer({ text, compact }: { text: string; compact?: boolean }) {
  const lines = text.split("\n");
  const elements: React.ReactNode[] = [];
  let i = 0;

  while (i < lines.length) {
    const line = lines[i];
    const textCls = compact ? TEXT_COMPACT : TEXT_CLASS;

    // Blank line
    if (line.trim() === "") {
      elements.push(<div key={i} className="h-2" />);
      i++;
      continue;
    }

    // Horizontal rule
    if (/^---+$/.test(line.trim())) {
      elements.push(<hr key={i} className="border-white/5 my-2" />);
      i++;
      continue;
    }

    // Heading
    const h2Match = line.match(/^## (.+)/);
    if (h2Match) {
      elements.push(
        <h3 key={i} className={`${compact ? "text-[11px]" : "text-xs"} font-bold text-white/65 uppercase tracking-wider mt-3 mb-1`}>
          <InlineMarkdown text={h2Match[1]} />
        </h3>,
      );
      i++;
      continue;
    }
    const h3Match = line.match(/^### (.+)/);
    if (h3Match) {
      elements.push(
        <h4 key={i} className={`${compact ? "text-[10px]" : "text-[11px]"} font-semibold text-white/50 mt-2 mb-0.5`}>
          <InlineMarkdown text={h3Match[1]} />
        </h4>,
      );
      i++;
      continue;
    }

    // Table — collect consecutive table lines
    if (/^\|.+\|$/.test(line.trim())) {
      const tableLines: string[] = [];
      while (i < lines.length && /^\|.+\|$/.test(lines[i].trim())) {
        tableLines.push(lines[i].trim());
        i++;
      }
      // filter separator line (|---|---|)
      const dataLines = tableLines.filter((l) => !/^\|[\s\-:|]+\|$/.test(l));
      if (dataLines.length > 0) {
        const headers = dataLines[0]
          .split("|")
          .slice(1, -1)
          .map((c) => c.trim());
        const rows = dataLines.slice(1);
        elements.push(
          <table
            key={`tbl-${i - tableLines.length}`}
            className={`${compact ? "text-[8px]" : "text-[10px]"} w-full border-collapse mt-1 mb-2`}
          >
            <thead>
              <tr className="border-b border-white/5">
                {headers.map((h, hi) => (
                  <th key={hi} className="text-left py-1 px-2 text-white/30 font-medium uppercase tracking-wider">
                    <InlineMarkdown text={h} />
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {rows.map((row, ri) => {
                const cells = row.split("|").slice(1, -1).map((c) => c.trim());
                return (
                  <tr key={ri} className="border-b border-white/[0.02]">
                    {cells.map((cell, ci) => (
                      <td key={ci} className="py-1 px-2 text-white/40">
                        <InlineMarkdown text={cell} />
                      </td>
                    ))}
                  </tr>
                );
              })}
            </tbody>
          </table>,
        );
      }
      continue;
    }

    // Blockquote
    const bqMatch = line.match(/^>\s?(.*)/);
    if (bqMatch) {
      elements.push(
        <div key={i} className={`border-l-2 border-[#eab308]/30 pl-2.5 my-1 ${compact ? "text-[9px]" : "text-[10px]"} text-white/30 italic`}>
          <InlineMarkdown text={bqMatch[1]} />
        </div>,
      );
      i++;
      continue;
    }

    // Bullet list — collect consecutive list items
    if (/^-\s/.test(line)) {
      const listItems: string[] = [];
      while (i < lines.length && /^-\s/.test(lines[i])) {
        listItems.push(lines[i].replace(/^-\s/, ""));
        i++;
      }
      elements.push(
        <ul key={`ul-${i - listItems.length}`} className="list-disc list-inside my-1 space-y-0.5">
          {listItems.map((item, li) => (
            <li key={li} className={compact ? "text-[9px] text-white/35" : "text-[10px] text-white/40"}>
              <InlineMarkdown text={item} />
            </li>
          ))}
        </ul>,
      );
      continue;
    }

    // Regular paragraph / key-value line
    elements.push(
      <p key={i} className={textCls}>
        <InlineMarkdown text={line} />
      </p>,
    );
    i++;
  }

  return <div className="overflow-auto max-h-[300px]">{elements}</div>;
}

function isMarkdown(text: string): boolean {
  return /^#{1,4}\s|\*\*|`[^`]+`|^\|.*\|$|^-\s|^>\s|^---+$/m.test(text);
}

function renderToolOutput(output: unknown, compact?: boolean) {
  if (output === null || output === undefined) return null;
  if (typeof output !== "string") {
    return (
      <JsonExplorer
        data={JSON.stringify(output)}
        isWrapping={true}
        className={`overflow-auto ${compact ? "max-h-[100px]" : "max-h-[150px]"}`}
      />
    );
  }
  try {
    const parsed = JSON.parse(output);
    if (parsed !== null && typeof parsed === "object") {
      return (
        <div
          className={`debug-scrollbar bg-transparent overflow-auto ${compact ? "max-h-[100px]" : "max-h-[150px]"}`}
        >
          <JsonNode value={parsed} depth={0} isWrapping={true} />
        </div>
      );
    }
  } catch {}
  if (isMarkdown(output)) {
    return <MarkdownRenderer text={output} compact={compact} />;
  }
  return (
    <div
      className={`whitespace-pre-wrap break-words leading-relaxed ${
        compact ? "text-[10px]" : "text-[11px]"
      } ${output.includes("SUCCESS") ? "text-[#98c379]" : "text-white/40"}`}
    >
      {output}
    </div>
  );
}

function ToolInputRenderer({
  call,
  isWrapping,
}: {
  call: { toolName: string; input: Record<string, unknown> };
  isWrapping: boolean;
}) {
  const { toolName, input } = call;

  if (toolName === TOOL_NAMES.GENERATE_DIALOGUE) {
    return (
      <div>
        <div className="mb-2">
          <span className="text-white/40">
            {(input.messages as any[] | undefined)?.length ?? 0} messages,{" "}
          </span>
          <span className="text-white/40">
            {(input.options as any[] | undefined)?.length ?? 0} options
          </span>
        </div>
        <JsonExplorer
          data={JSON.stringify(input)}
          isWrapping={isWrapping}
          className="max-h-[300px] overflow-auto"
        />
      </div>
    );
  }

  if (toolName === TOOL_NAMES.UPDATE_ENTITY) {
    return (
      <div>
        <div className="mb-2 text-white/40">
          <span className="text-entity-object font-mono">{(input.id as string) ?? "?"}</span>
          {input.shortDescription != null && (
            <span className="text-white/30"> shortDescription</span>
          )}
          {input.longDescription != null && <span className="text-white/30"> longDescription</span>}
          {input.attributes != null && <span className="text-white/30"> attributes</span>}
          {input.opinions != null && <span className="text-white/30"> opinions</span>}
        </div>
        <JsonExplorer
          data={JSON.stringify(input)}
          isWrapping={isWrapping}
          className="max-h-[200px] overflow-auto"
        />
      </div>
    );
  }

  if (toolName === TOOL_NAMES.UPDATE_PLOT) {
    return (
      <div>
        <span className="text-entity-object">{input.id as string}</span>
        <span className="text-white/20"> → </span>
        <span className="text-[#98c379]">{input.status as string}</span>
      </div>
    );
  }

  if (toolName === TOOL_NAMES.CREATE_PLOT) {
    return (
      <div>
        <div className="text-white/60">"{input.title as string}"</div>
        {input.description && (
          <div className="text-white/30 mt-1">{input.description as string}</div>
        )}
      </div>
    );
  }

  if (toolName === TOOL_NAMES.LIST_ENTITIES) {
    return (
      <div>
        <span className="text-white/40">Fetch </span>
        <span className="text-entity-object">
          {input.type ? (input.type as string) : "all types"}
        </span>
      </div>
    );
  }

  if (toolName === TOOL_NAMES.GET_ENTITY) {
    const inputAny = input as any;
    return (
      <div>
        {inputAny.id ? (
          <>
            <span className="text-white/40">ID: </span>
            <span className="text-entity-object font-mono">{inputAny.id as string}</span>
          </>
        ) : inputAny.ids ? (
          <>
            <span className="text-white/40">Bulk ({inputAny.ids.length}): </span>
            <span className="text-entity-object font-mono">
              [{(inputAny.ids as string[]).slice(0, 3).join(", ")}
              {inputAny.ids.length > 3 ? `, +${inputAny.ids.length - 3} more` : ""}]
            </span>
          </>
        ) : inputAny.search ? (
          <>
            <span className="text-white/40">Search: </span>
            <span className="text-[#98c379]">"{inputAny.search as string}"</span>
          </>
        ) : (
          <span className="text-[#e06c75]">Missing query params</span>
        )}
      </div>
    );
  }

  if (toolName === TOOL_NAMES.GET_PLOT) {
    const inputAny = input as any;
    return (
      <div>
        {inputAny.id ? (
          <>
            <span className="text-white/40">Plot: </span>
            <span className="text-entity-object font-mono">{inputAny.id as string}</span>
          </>
        ) : inputAny.ids ? (
          <>
            <span className="text-white/40">Bulk ({inputAny.ids.length}): </span>
            <span className="text-entity-object font-mono">
              [{(inputAny.ids as string[]).slice(0, 3).join(", ")}
              {inputAny.ids.length > 3 ? `, +${inputAny.ids.length - 3} more` : ""}]
            </span>
          </>
        ) : inputAny.status ? (
          <>
            <span className="text-white/40">Status filter: </span>
            <span className="text-[#eab308]">{inputAny.status as string}</span>
          </>
        ) : (
          <span className="text-white/40">All plots</span>
        )}
      </div>
    );
  }

  return (
    <JsonExplorer
      data={JSON.stringify(input)}
      isWrapping={isWrapping}
      className="max-h-[200px] overflow-auto"
    />
  );
}

/** Compact variant for child/assistant traces — less verbose input rendering. */
function ToolInputRendererCompact({
  call,
}: {
  call: { toolName: string; input: Record<string, unknown> };
}) {
  const { toolName, input } = call;
  const inputAny = input as any;

  if (toolName === TOOL_NAMES.GENERATE_DIALOGUE) {
    return (
      <div className="text-[10px]">
        <span className="text-white/30">
          {(input.messages as any[] | undefined)?.length ?? 0} msgs,{" "}
        </span>
        <span className="text-white/30">
          {(input.options as any[] | undefined)?.length ?? 0} opts
        </span>
      </div>
    );
  }
  if (toolName === TOOL_NAMES.UPDATE_ENTITY) {
    return (
      <div className="text-[10px]">
        <span className="text-entity-object font-mono">{(input.id as string) ?? "?"}</span>
        {input.shortDescription != null && <span className="text-white/30"> shortDesc</span>}
        {input.longDescription != null && <span className="text-white/30"> longDesc</span>}
        {input.attributes != null && <span className="text-white/30"> attrs</span>}
        {input.opinions != null && <span className="text-white/30"> opinions</span>}
      </div>
    );
  }
  if (toolName === TOOL_NAMES.UPDATE_PLOT) {
    return (
      <div className="text-[10px]">
        <span className="text-entity-object">{input.id as string}</span>
        <span className="text-white/20"> → </span>
        <span className="text-[#98c379]">{input.status as string}</span>
      </div>
    );
  }
  if (toolName === TOOL_NAMES.CREATE_PLOT) {
    return (
      <div className="text-[10px]">
        <div className="text-white/50">"{input.title as string}"</div>
      </div>
    );
  }
  if (toolName === TOOL_NAMES.LIST_ENTITIES) {
    return (
      <div className="text-[10px]">
        <span className="text-white/30">Fetch </span>
        <span className="text-entity-object">
          {input.type ? (input.type as string) : "all types"}
        </span>
      </div>
    );
  }
  if (toolName === TOOL_NAMES.GET_ENTITY) {
    return (
      <div className="text-[10px]">
        {inputAny.id ? (
          <>
            <span className="text-white/30">ID: </span>
            <span className="text-entity-object font-mono">{inputAny.id as string}</span>
          </>
        ) : inputAny.ids ? (
          <>
            <span className="text-white/30">Bulk ({inputAny.ids.length}): </span>
            <span className="text-entity-object font-mono">
              [{(inputAny.ids as string[]).slice(0, 3).join(", ")}
              {inputAny.ids.length > 3 ? `, +${inputAny.ids.length - 3} more` : ""}]
            </span>
          </>
        ) : inputAny.search ? (
          <>
            <span className="text-white/30">Search: </span>
            <span className="text-[#98c379]">"{inputAny.search as string}"</span>
          </>
        ) : (
          <span className="text-[#e06c75]">Missing params</span>
        )}
      </div>
    );
  }
  if (toolName === TOOL_NAMES.GET_PLOT) {
    return (
      <div className="text-[10px]">
        {inputAny.id ? (
          <>
            <span className="text-white/30">Plot: </span>
            <span className="text-entity-object font-mono">{inputAny.id as string}</span>
          </>
        ) : inputAny.ids ? (
          <>
            <span className="text-white/30">Bulk ({inputAny.ids.length}): </span>
            <span className="text-entity-object font-mono">
              [{(inputAny.ids as string[]).slice(0, 3).join(", ")}
              {inputAny.ids.length > 3 ? `, +${inputAny.ids.length - 3} more` : ""}]
            </span>
          </>
        ) : inputAny.status ? (
          <>
            <span className="text-white/30">Status: </span>
            <span className="text-[#eab308]">{inputAny.status as string}</span>
          </>
        ) : (
          <span className="text-white/30">All plots</span>
        )}
      </div>
    );
  }
  return (
    <JsonExplorer
      data={JSON.stringify(input)}
      isWrapping={true}
      className="max-h-[150px] overflow-auto"
    />
  );
}

function toolColor(toolName: string) {
  if (toolName === TOOL_NAMES.GENERATE_DIALOGUE) return "bg-entity-character";
  if (toolName === TOOL_NAMES.UPDATE_ENTITY) return "bg-entity-location";
  if (toolName === TOOL_NAMES.UPDATE_PLOT) return "bg-[#eab308]";
  if (toolName === TOOL_NAMES.CREATE_PLOT) return "bg-[#98c379]";
  return "bg-white/20";
}

interface ToolCallCardProps {
  call: {
    toolName: string;
    input?: Record<string, unknown>;
    args?: Record<string, unknown>;
    output?: unknown;
  };
  isWrapping: boolean;
  compact?: boolean;
}

export const ToolCallCard: React.FC<ToolCallCardProps> = ({ call, isWrapping, compact }) => {
  const input = (call.input || call.args || {}) as Record<string, unknown>;
  const callIsError = isToolError(call.output);

  if (compact) {
    return (
      <div
        className={`mb-1 p-2 rounded-sm ${
          callIsError
            ? "bg-red-500/[0.04] border border-red-500/30"
            : "bg-white/[0.02] border border-white/5"
        }`}
      >
        <div className="flex items-center justify-between">
          <span className="text-[10px] font-bold text-white/30 uppercase tracking-wider flex items-center gap-2">
            {readableToolName(call.toolName)}
            {callIsError && (
              <span className="px-1 py-[1px] rounded-sm text-[7px] font-bold uppercase tracking-widest bg-red-500/20 text-red-400 border border-red-500/30 leading-none">
                Error
              </span>
            )}
          </span>
          <CopyButton content={JSON.stringify(input, null, 2)} />
        </div>
        <div className="mt-1">
          <ToolInputRendererCompact call={{ toolName: call.toolName, input }} />
        </div>
        {call.output != null && (
          <div className="mt-1 pt-1 border-t border-white/5">
            {callIsError ? (
              <div className="flex items-start gap-1.5">
                <span className="mt-[2px] shrink-0 w-3 h-3 rounded-full bg-red-500/20 border border-red-500/30 flex items-center justify-center">
                  <span className="text-[6px] text-red-400 font-bold leading-none">!</span>
                </span>
                <div className="min-w-0 flex-1">
                  <span className="text-[8px] font-bold text-red-400 uppercase tracking-wider">
                    Error
                  </span>
                  <div className="mt-0.5 text-[10px] whitespace-pre-wrap break-words leading-relaxed text-red-400/90 font-mono bg-red-500/[0.03] p-1.5 rounded-sm border border-red-500/10">
                    {String(call.output)}
                  </div>
                </div>
              </div>
            ) : (
              <>{renderToolOutput(call.output, true)}</>
            )}
          </div>
        )}
      </div>
    );
  }

  return (
    <div
      className={`mb-2 p-3 rounded-sm ${
        callIsError
          ? "bg-red-500/[0.04] border border-red-500/30"
          : "bg-white/[0.02] border border-white/5"
      }`}
    >
      <div className="flex items-center justify-between mb-2">
        <h4 className="text-[10px] font-bold text-white/40 uppercase tracking-[0.15em] flex items-center gap-2">
          <div className={`w-[1px] h-3 ${toolColor(call.toolName)}`} />
          {readableToolName(call.toolName)}
          {callIsError && (
            <span className="px-1 py-[1px] rounded-sm text-[8px] font-bold uppercase tracking-widest bg-red-500/20 text-red-400 border border-red-500/30 leading-none">
              Error
            </span>
          )}
        </h4>
        <CopyButton content={JSON.stringify(input, null, 2)} />
      </div>

      <div className="text-[11px] text-white/50 font-mono">
        <ToolInputRenderer call={{ toolName: call.toolName, input }} isWrapping={isWrapping} />
      </div>

      {call.output != null && (
        <div className="mt-2 pt-2 border-t border-white/5">
          {callIsError ? (
            <div className="flex items-start gap-2">
              <span className="mt-[1px] shrink-0 w-3.5 h-3.5 rounded-full bg-red-500/20 border border-red-500/30 flex items-center justify-center">
                <span className="text-[8px] text-red-400 font-bold leading-none">!</span>
              </span>
              <div className="min-w-0 flex-1">
                <span className="text-[9px] font-bold text-red-400 uppercase tracking-wider">
                  Error
                </span>
                <div className="mt-1">
                  <div className="text-[11px] whitespace-pre-wrap break-words leading-relaxed text-red-400/90 font-mono bg-red-500/[0.03] p-2 rounded-sm border border-red-500/10">
                    {String(call.output)}
                  </div>
                </div>
              </div>
            </div>
          ) : (
            <>
              <span className="text-[9px] font-bold text-white/15 uppercase tracking-wider">
                Result
              </span>
              <div className="mt-1">{renderToolOutput(call.output)}</div>
            </>
          )}
        </div>
      )}
    </div>
  );
};
