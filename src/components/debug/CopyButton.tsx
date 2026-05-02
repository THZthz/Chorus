import React, { useState } from "react";
import { Check, Copy } from "lucide-react";

export const CopyButton: React.FC<{ content: string }> = ({ content }) => {
  const [copied, setCopied] = useState(false);

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(content);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch (err) {
      console.error("Failed to copy!", err);
    }
  };

  return (
    <button
      onClick={handleCopy}
      className={`p-1 flex items-center gap-1.5 transition-all ${
        copied ? "text-[#a3c2a3]" : "text-white/40 hover:text-white"
      }`}
      title={copied ? "Copied!" : "Copy to clipboard"}
    >
      {copied ? <Check size={12} /> : <Copy size={12} />}
      <span className="text-[10px] font-bold uppercase tracking-wider">
        {copied ? "Copied" : "Copy"}
      </span>
    </button>
  );
};
