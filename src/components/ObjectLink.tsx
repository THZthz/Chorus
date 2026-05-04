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
