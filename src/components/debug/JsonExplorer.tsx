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

import React, { useEffect, useState } from "react";
import { JsonNode } from "@/components/debug/JsonNode";

export const JsonExplorer: React.FC<{
  data: string | null;
  isWrapping?: boolean;
  className?: string;
}> = ({ data, isWrapping = true, className = "h-full overflow-auto" }) => {
  const [parsed, setParsed] = useState<any>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!data) return;
    try {
      setParsed(JSON.parse(data));
      setError(null);
    } catch (e) {
      setError(data);
    }
  }, [data]);

  if (!data)
    return (
      <div className="p-4 text-white/30 italic text-[10px] uppercase tracking-widest">
        Empty_Transmission
      </div>
    );

  if (error) {
    return (
      <div className={`p-3 debug-scrollbar bg-transparent ${className}`}>
        <pre
          className={`text-[#e06c75] text-[11px] font-mono leading-relaxed ${isWrapping ? "whitespace-pre-wrap" : "whitespace-pre"}`}
        >
          {error}
        </pre>
      </div>
    );
  }

  if (!parsed) return null;

  return (
    <div className={`p-3 debug-scrollbar bg-transparent ${className}`}>
      <div className={!isWrapping ? "w-fit min-w-full" : ""}>
        <JsonNode value={parsed} depth={0} isWrapping={isWrapping} />
      </div>
    </div>
  );
};
