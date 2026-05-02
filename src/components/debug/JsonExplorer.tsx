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
