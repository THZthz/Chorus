import React, { useState, useRef, useLayoutEffect } from 'react';
import { motion, AnimatePresence } from 'motion/react';
import { ChevronDown, ChevronUp, Info, User, MapPin, Box } from 'lucide-react';
import { WorldEntity } from '@/types/entities';

interface Props {
  object: WorldEntity;
  onClose?: () => void;
}

export const ObjectTooltip: React.FC<Props> = ({ object }) => {
  const [showLongDesc, setShowLongDesc] = useState(false);
  const [position, setPosition] = useState<{ vertical: 'above' | 'below', horizontal: 'left' | 'right' }>({
    vertical: 'above',
    horizontal: 'left'
  });
  const tooltipRef = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    if (tooltipRef.current) {
      const rect = tooltipRef.current.getBoundingClientRect();
      const viewportWidth = window.innerWidth;

      let vertical: 'above' | 'below' = position.vertical;
      let horizontal: 'left' | 'right' = position.horizontal;

      // Check vertical space on mount for the initial appearance
      // If the collapsed tooltip is already hitting the top of the viewport, flip it below.
      if (rect.top < 0) {
        vertical = 'below';
      }

      // Check horizontal space
      if (rect.right > viewportWidth) {
        horizontal = 'right';
      } else if (rect.left < 0) {
        horizontal = 'left';
      }

      setPosition({ vertical, horizontal });
    }
  }, []); // Run ONLY once on mount to keep position stable during interaction

  const Icon = object.type === 'CHARACTER' ? User : object.type === 'LOCATION' ? MapPin : Box;
  const typeColor = object.type === 'CHARACTER' ? 'text-pink-500' : object.type === 'LOCATION' ? 'text-green-500' : 'text-cyan-500';

  const positionClasses = [
    position.vertical === 'above' ? 'bottom-full mb-2' : 'top-full mt-2',
    position.horizontal === 'left' ? 'left-0' : 'right-0'
  ].join(' ');

  const yOffset = position.vertical === 'above' ? 10 : -10;

  return (
    <motion.div
      ref={tooltipRef}
      initial={{ opacity: 0, scale: 0.9, y: yOffset }}
      animate={{ opacity: 1, scale: 1, y: 0 }}
      exit={{ opacity: 0, scale: 0.9, y: yOffset }}
      className={`absolute z-[100] w-72 overflow-visible ${positionClasses}`}
      id={`object-tooltip-${object.id}`}
    >
      <div className="bg-[#0a0a0a]/95 backdrop-blur-md border border-gray-800 rounded-lg shadow-2xl p-4 overflow-hidden">
        {/* Header */}
        <div className="flex items-start justify-between mb-3 border-b border-gray-800 pb-2">
          <div>
            <h3 className="text-white font-serif font-bold text-lg leading-tight">
              {object.displayName}
            </h3>
            <div className="flex items-center gap-2 mt-1">
              <span className={`text-[10px] uppercase tracking-[0.2em] font-mono ${typeColor}`}>
                {object.type}
              </span>
              <span className="text-[10px] text-gray-500 uppercase tracking-[0.2em] font-mono">
                {object.id}
              </span>
            </div>
          </div>
          <Icon size={16} className="text-gray-600" />
        </div>

        {/* Short Description */}
        <p className="text-gray-300 text-sm leading-relaxed mb-4">
          {object.shortDescription}
        </p>

        {/* Character Opinions */}
        {object.type === 'CHARACTER' && object.opinions && Object.keys(object.opinions).length > 0 && (
          <div className="mb-4 space-y-2">
            <div className="text-[10px] text-gray-500 uppercase tracking-widest font-mono">Opinions</div>
            {Object.entries(object.opinions).map(([id, opinion]) => (
              <div key={id} className="text-[11px] text-pink-200/70 italic bg-pink-900/10 p-2 rounded border border-pink-900/20">
                <span className="font-bold uppercase tracking-tighter mr-1">{id}:</span>
                "{opinion}"
              </div>
            ))}
          </div>
        )}

        {/* Attributes Grid */}
        <div className="grid grid-cols-1 gap-2 mb-4 bg-black/40 p-2 rounded border border-gray-900">
          {Object.entries(object.attributes).map(([key, value]) => (
            <div key={key} className="flex justify-between items-baseline text-[11px]">
              <span className="text-gray-500 uppercase tracking-wider font-mono italic">{key}</span>
              <span className="text-gray-200 font-medium">{value}</span>
            </div>
          ))}
        </div>

        {/* Expandable Long Description */}
        <button
          onClick={(e) => {
            e.stopPropagation();
            setShowLongDesc(!showLongDesc);
          }}
          className="w-full flex items-center justify-between text-[11px] text-gray-400 hover:text-white transition-colors py-1 px-2 bg-gray-900/50 rounded border border-gray-800"
        >
          <span>{showLongDesc ? 'HIDE DETAILS' : 'VIEW FULL DESCRIPTION'}</span>
          {showLongDesc ? <ChevronUp size={12} /> : <ChevronDown size={12} />}
        </button>

        <AnimatePresence>
          {showLongDesc && (
            <motion.div
              initial={{ height: 0, opacity: 0 }}
              animate={{ height: 'auto', opacity: 1 }}
              exit={{ height: 0, opacity: 0 }}
              className="overflow-hidden"
            >
              <div className="pt-3 text-gray-400 text-xs italic leading-relaxed font-serif">
                {object.longDescription}
              </div>
            </motion.div>
          )}
        </AnimatePresence>

        {/* Decorative pulse element */}
        <div className="absolute top-0 right-0 w-1 h-1 bg-cyan-500 rounded-full animate-pulse m-2 opacity-50" />
      </div>
    </motion.div>
  );
};
