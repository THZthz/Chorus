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

import React, { useState, useEffect, useLayoutEffect, useRef, useCallback } from "react";
import { ChevronDown } from "lucide-react";
import { createPortal } from "react-dom";

// ── CustomSelect ──────────────────────────────────────────────────────────────

interface SelectOption {
  value: string;
  label: string;
}

export const CustomSelect: React.FC<{
  value: string;
  options: (string | SelectOption)[];
  onChange: (value: string) => void;
  className?: string;
}> = ({ value, options, onChange, className = "" }) => {
  const [open, setOpen] = useState(false);
  const [dropPos, setDropPos] = useState<{ top: number; left: number; width: number } | null>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const dropRef = useRef<HTMLDivElement>(null);

  const normalized = options.map((o) => (typeof o === "string" ? { value: o, label: o } : o));
  const selected = normalized.find((o) => o.value === value);

  const toggleOpen = (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    if (open) {
      setOpen(false);
      return;
    }
    const rect = buttonRef.current?.getBoundingClientRect();
    if (rect) setDropPos({ top: rect.bottom + 2, left: rect.left, width: rect.width });
    setOpen(true);
  };

  useEffect(() => {
    if (!open) return;
    const handler = (e: MouseEvent) => {
      if (
        dropRef.current &&
        !dropRef.current.contains(e.target as Node) &&
        buttonRef.current &&
        !buttonRef.current.contains(e.target as Node)
      ) {
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, [open]);

  return (
    <div className={`relative ${className}`}>
      <button
        ref={buttonRef}
        type="button"
        onMouseDown={toggleOpen}
        className="w-full flex items-center justify-between bg-white/[0.04] border border-white/10 rounded-sm px-2 py-1 text-[11px] font-mono text-white/70 hover:border-white/20 focus:outline-none focus-visible:ring-1 focus-visible:ring-accent/50 transition-colors"
      >
        <span className="truncate">{selected?.label ?? value}</span>
        <ChevronDown
          size={9}
          className={`flex-shrink-0 text-white/30 ml-1 transition-transform duration-100 ${open ? "rotate-180" : ""}`}
        />
      </button>
      {open &&
        dropPos &&
        createPortal(
          <div
            ref={dropRef}
            style={{
              position: "fixed",
              top: dropPos.top,
              left: dropPos.left,
              width: dropPos.width,
              zIndex: 9999,
            }}
            className="bg-[#111214] border border-white/15 rounded-sm shadow-2xl py-0.5"
          >
            {normalized.map((opt) => (
              <button
                key={opt.value}
                type="button"
                onMouseDown={(e) => {
                  e.preventDefault();
                  onChange(opt.value);
                  setOpen(false);
                }}
                className={`w-full text-left px-2 py-1.5 text-[11px] font-mono transition-colors focus-visible:ring-1 focus-visible:ring-accent/50 focus-visible:ring-inset ${
                  opt.value === value
                    ? "bg-white/8 text-white/90"
                    : "text-white/55 hover:bg-white/[0.05] hover:text-white/80"
                }`}
              >
                {opt.label}
              </button>
            ))}
          </div>,
          document.body,
        )}
    </div>
  );
};

// ── ResizableTextarea ─────────────────────────────────────────────────────────

export const ResizableTextarea: React.FC<
  React.TextareaHTMLAttributes<HTMLTextAreaElement> & {
    minHeight?: number;
    initialHeight?: number;
  }
> = ({
  minHeight = 72,
  initialHeight,
  className = "",
  style,
  onChange,
  value,
  defaultValue,
  ...props
}) => {
  const [maxH, setMaxH] = useState(initialHeight ?? minHeight);
  const startYRef = useRef(0);
  const startHRef = useRef(0);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const adjustHeight = useCallback(() => {
    const el = textareaRef.current;
    if (!el) return;
    el.style.overflowY = "scroll";
    el.style.height = "auto";
    const sh = el.scrollHeight;
    el.style.height = `${Math.max(minHeight, sh)}px`;
    el.style.overflowY = "hidden";
  }, [minHeight]);

  useLayoutEffect(() => {
    adjustHeight();
  }, [value, adjustHeight]);

  const handleChange = useCallback(
    (e: React.ChangeEvent<HTMLTextAreaElement>) => {
      adjustHeight();
      onChange?.(e);
    },
    [adjustHeight, onChange],
  );

  const onHandleDown = (e: React.MouseEvent) => {
    startYRef.current = e.clientY;
    startHRef.current = maxH;
    e.preventDefault();
    const onMove = (ev: MouseEvent) => {
      const delta = ev.clientY - startYRef.current;
      setMaxH(Math.max(minHeight, startHRef.current + delta));
    };
    const onUp = () => {
      document.removeEventListener("mousemove", onMove);
      document.removeEventListener("mouseup", onUp);
    };
    document.addEventListener("mousemove", onMove);
    document.addEventListener("mouseup", onUp);
  };

  return (
    <div className="flex flex-col group/rta">
      <div className="debug-scrollbar" style={{ maxHeight: maxH, overflowY: "auto" }}>
        <textarea
          ref={textareaRef}
          className={className}
          style={{ ...style, resize: "none", overflow: "hidden" }}
          value={value}
          defaultValue={defaultValue}
          onChange={handleChange}
          {...props}
        />
      </div>
      <div
        onMouseDown={onHandleDown}
        className="h-2 flex items-center justify-center cursor-ns-resize select-none"
      >
        <div className="w-8 h-px bg-white/10 group-hover/rta:bg-white/30 transition-colors" />
      </div>
    </div>
  );
};
