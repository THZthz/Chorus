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

export const DieFace: React.FC<{ value: number; size?: "sm" | "md" | "lg" | "xs" }> = ({
  value,
  size = "md",
}) => {
  const dotPositions: Record<number, number[]> = {
    1: [4],
    2: [2, 6],
    3: [2, 4, 6],
    4: [0, 2, 6, 8],
    5: [0, 2, 4, 6, 8],
    6: [0, 3, 6, 2, 5, 8],
  };

  const dots = dotPositions[value] || [];
  const sizes = {
    xs: { container: "w-3 h-3", dot: "w-0.5 h-0.5", gap: "gap-0" },
    sm: { container: "w-5 h-5", dot: "w-1 h-1", gap: "gap-0.5" },
    md: { container: "w-8 h-8", dot: "w-1.5 h-1.5", gap: "gap-1" },
    lg: { container: "w-12 h-12", dot: "w-2 h-2", gap: "gap-1.5" },
  };

  const s = sizes[size];
  return (
    <div className={`grid grid-cols-3 grid-rows-3 ${s.gap} ${s.container} pointer-events-none`}>
      {[...Array(9)].map((_, i) => (
        <div key={i} className="flex items-center justify-center">
          {dots.includes(i) && (
            <div
              className={`${s.dot} rounded-full bg-white shadow-[0_0_2px_rgba(255,255,255,0.5)]`}
            />
          )}
        </div>
      ))}
    </div>
  );
};
