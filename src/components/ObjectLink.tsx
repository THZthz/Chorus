import React, { useState, useRef, useEffect } from "react";
import { AnimatePresence } from "motion/react";
import { worldManager } from "@/services/WorldManager";
import { ObjectTooltip } from "@/components/ObjectTooltip";

interface Props {
  displayName: string;
  objectId: string;
}

export const ObjectLink: React.FC<Props> = ({ displayName, objectId }) => {
  const [isOpen, setIsOpen] = useState(false);
  const object = worldManager.getEntity(objectId);
  const containerRef = useRef<HTMLSpanElement>(null);

  // Close on outside click
  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (containerRef.current && !containerRef.current.contains(event.target as Node)) {
        setIsOpen(false);
      }
    };

    if (isOpen) {
      document.addEventListener("mousedown", handleClickOutside);
    }
    return () => {
      document.removeEventListener("mousedown", handleClickOutside);
    };
  }, [isOpen]);

  if (!object) {
    return <span className="text-red-400 underline decoration-dotted">[{displayName}]</span>;
  }

  return (
    <span
      ref={containerRef}
      onMouseEnter={() => setIsOpen(true)}
      onMouseLeave={() => setIsOpen(false)}
      className="relative inline-block"
    >
      <button
        onClick={() => setIsOpen(!isOpen)}
        className="text-cyan-400 hover:text-cyan-300 underline decoration-cyan-900 underline-offset-4 decoration-2 transition-all cursor-help font-medium"
      >
        {displayName}
      </button>

      <AnimatePresence>{isOpen && <ObjectTooltip object={object} />}</AnimatePresence>
    </span>
  );
};
