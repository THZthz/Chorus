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

import React, { useState } from "react";
import { ChevronDown } from "lucide-react";

const MAX_DEPTH = 10;

export const JsonNode: React.FC<{
  label?: string;
  value: any;
  depth: number;
  isLast?: boolean;
  isWrapping?: boolean;
}> = ({ label, value, depth, isLast = true, isWrapping = true }) => {
  const [isExpanded, setIsExpanded] = useState(depth < 1);
  const hasChildren = value !== null && typeof value === "object";
  const isEmpty =
    hasChildren && (Array.isArray(value) ? value.length === 0 : Object.keys(value).length === 0);
  const isMaxDepth = depth >= MAX_DEPTH;

  const renderValue = () => {
    if (value === null) return <span className="text-[#5c6370]">null</span>;
    if (typeof value === "string") {
      return (
        <span
          className={`text-[#98c379] ${isWrapping ? "whitespace-pre-wrap break-words" : "whitespace-pre"}`}
        >
          "{value}"
        </span>
      );
    }
    if (typeof value === "number") return <span className="text-[#d19a66]">{value}</span>;
    if (typeof value === "boolean")
      return <span className="text-[#c678dd] font-bold">{value.toString()}</span>;

    if (Array.isArray(value)) {
      if (isEmpty) return <span className="text-[#abb2bf]/20">[]</span>;
      if (isMaxDepth) return <span className="text-[#5c6370]">[{value.length} items]</span>;

      return isExpanded ? (
        <span>
          <span className="text-[#abb2bf]/70">[</span>
          <div className="pl-4 border-l border-white/[0.03] ml-1.5 my-0.5">
            {value.map((v, i) => (
              <JsonNode
                key={i}
                value={v}
                depth={depth + 1}
                isLast={i === value.length - 1}
                isWrapping={isWrapping}
              />
            ))}
          </div>
          <span className="text-[#abb2bf]/70">]</span>
        </span>
      ) : (
        <button
          onClick={() => setIsExpanded(true)}
          className="text-[#5c6370] hover:text-[#61afef] bg-white/[0.03] px-1 rounded transition-colors text-[10px]"
        >
          [{value.length} items]
        </button>
      );
    }

    // Object
    if (isEmpty) return <span className="text-[#abb2bf]/20">{"{}"}</span>;
    if (isMaxDepth) return <span className="text-[#5c6370]">{"{…}"}</span>;

    return isExpanded ? (
      <span>
        <span className="text-[#abb2bf]/70">{"{"}</span>
        <div className="pl-4 border-l border-white/[0.03] ml-1.5 my-0.5">
          {Object.entries(value).map(([k, v], i, arr) => (
            <JsonNode
              key={k}
              label={k}
              value={v}
              depth={depth + 1}
              isLast={i === arr.length - 1}
              isWrapping={isWrapping}
            />
          ))}
        </div>
        <span className="text-[#abb2bf]/70">{"}"}</span>
      </span>
    ) : (
      <button
        onClick={() => setIsExpanded(true)}
        className="text-[#5c6370] hover:text-[#61afef] bg-white/[0.03] px-1 rounded transition-colors text-[10px]"
      >
        {"{ " +
          Object.keys(value).slice(0, 2).join(", ") +
          (Object.keys(value).length > 2 ? "..." : "") +
          " }"}
      </button>
    );
  };

  return (
    <div className="font-mono text-[11px] leading-relaxed group/node">
      <div className="flex items-start">
        <div className="w-4 h-4 flex items-center justify-center flex-shrink-0 mt-0.5">
          {hasChildren && !isEmpty && !isMaxDepth && (
            <button
              onClick={() => setIsExpanded(!isExpanded)}
              className={`text-gray-500 hover:text-blue-400 transition-transform duration-200 ${isExpanded ? "rotate-0" : "-rotate-90"}`}
            >
              <ChevronDown size={12} />
            </button>
          )}
        </div>

        <div className="flex-1 min-w-0">
          {label && (
            <span className="text-[#e06c75] mr-2 group-hover/node:text-[#e06c75] transition-colors">
              <span className="text-[#abb2bf]">"</span>
              {label}
              <span className="text-[#abb2bf]">"</span>
              <span className="text-[#abb2bf] ml-1">:</span>
            </span>
          )}
          {renderValue()}
          {!isLast && <span className="text-[#abb2bf]">,</span>}
        </div>
      </div>
    </div>
  );
};
