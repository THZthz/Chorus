import React, { useEffect, useState } from "react";
import {
  RefreshCw,
  Trash2,
  Monitor,
  WrapText,
  Search,
  Filter,
  Calendar,
  Clock,
} from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { consoleLogger, ConsoleLog, LogLevel } from "@/services/ConsoleLogger";
import { JsonNode } from "@/components/debug/JsonNode";

export const ConsoleViewer: React.FC = () => {
  const [logs, setLogs] = useState<ConsoleLog[]>(consoleLogger.getLogs());
  const [filterKeyword, setFilterKeyword] = useState("");
  const [filterLevels, setFilterLevels] = useState<LogLevel[]>(["log", "info", "warn", "error"]);
  const [dateStart, setDateStart] = useState<string>("");
  const [dateEnd, setDateEnd] = useState<string>("");
  const [showFilters, setShowFilters] = useState(false);
  const [isWrapping, setIsWrapping] = useState(true);

  const fetchPersistedLogs = async () => {
    try {
      const response = await fetch("/api/debug/console");
      if (response.ok) {
        const persistedLogs = await response.json();
        // Backend returns args as string, we need to parse it for the UI
        const formattedLogs = persistedLogs
          .map((log: any) => ({
            ...log,
            timestamp: new Date(log.timestamp.replace(" ", "T") + "Z").getTime(),
            args: JSON.parse(log.args),
          }))
          .reverse(); // DB returns DESC, in-memory expects ASC
        consoleLogger.setLogs(formattedLogs);
      }
    } catch (error) {
      console.error("Failed to fetch persisted console logs:", error);
    }
  };

  useEffect(() => {
    fetchPersistedLogs();
    const unsubscribe = consoleLogger.subscribe(() => {
      setLogs(consoleLogger.getLogs());
    });
    return unsubscribe;
  }, []);

  const clearLogs = () => {
    consoleLogger.clearLogs();
    setLogs([]);
  };

  const toggleLevel = (level: LogLevel) => {
    setFilterLevels((prev) =>
      prev.includes(level) ? prev.filter((l) => l !== level) : [...prev, level],
    );
  };

  const filteredLogs = logs.filter((log) => {
    if (!filterLevels.includes(log.level)) return false;

    if (filterKeyword) {
      try {
        const regex = new RegExp(filterKeyword, "i");
        // Test message or any of the stringified args
        const contentToTest = log.message;
        if (!regex.test(contentToTest)) return false;
      } catch (e) {
        if (!log.message.toLowerCase().includes(filterKeyword.toLowerCase())) return false;
      }
    }

    if (dateStart) {
      const start = new Date(dateStart).getTime();
      if (log.timestamp < start) return false;
    }
    if (dateEnd) {
      const end = new Date(dateEnd).getTime();
      if (log.timestamp > end) return false;
    }

    return true;
  });

  return (
    <div className="flex flex-col h-full overflow-hidden">
      <div className="flex items-center justify-between h-9 mb-6 flex-shrink-0">
        <div className="flex items-center gap-2 text-white/60">
          <Monitor size={16} />
          <h3 className="text-[10px] font-bold uppercase tracking-[0.2em]">CONSOLE_LOGS</h3>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={() => setIsWrapping(!isWrapping)}
            className={`flex items-center gap-2 px-3 py-1 rounded-sm border transition-all ${
              isWrapping
                ? "bg-white/10 text-white border-white/20"
                : "bg-white/5 text-white/40 border-white/10 hover:bg-white/10 hover:text-white/60"
            }`}
            title={isWrapping ? "Disable text wrap" : "Enable text wrap"}
          >
            <WrapText size={14} className={isWrapping ? "text-blue-400" : ""} />
            <span className="text-[10px] font-bold uppercase tracking-wider">Wrap</span>
          </button>
          <button
            onClick={() => setShowFilters(!showFilters)}
            className={`flex items-center gap-2 px-3 py-1 rounded-sm border transition-all ${
              showFilters
                ? "bg-white/10 text-white border-white/20"
                : "bg-white/5 text-white/40 border-white/10 hover:bg-white/10 hover:text-white/60"
            }`}
          >
            <Filter size={14} className={showFilters ? "text-[#ff6b35]" : ""} />
            <span className="text-[10px] font-bold uppercase tracking-wider">Filter</span>
          </button>
          <button
            onClick={fetchPersistedLogs}
            className="flex items-center gap-2 px-3 py-1 bg-white/5 text-white/40 hover:bg-white/10 hover:text-white rounded-sm border border-white/10 transition-all"
          >
            <RefreshCw size={14} />
            <span className="text-[10px] font-bold uppercase tracking-wider">Sync</span>
          </button>
          <button
            onClick={clearLogs}
            className="flex items-center gap-2 px-3 py-1 bg-white/5 text-white/40 hover:bg-red-500/20 hover:text-red-400 rounded-sm border border-white/5 hover:border-red-500/20 transition-all"
          >
            <Trash2 size={14} />
            <span className="text-[10px] font-bold uppercase tracking-wider">Clear</span>
          </button>
        </div>
      </div>

      <AnimatePresence>
        {showFilters && (
          <motion.div
            initial={{ height: 0, opacity: 0, marginBottom: 0 }}
            animate={{ height: "auto", opacity: 1, marginBottom: 24 }}
            exit={{ height: 0, opacity: 0, marginBottom: 0 }}
            className="flex flex-col gap-4 p-4 bg-white/[0.02] border border-white/5 rounded-sm overflow-hidden"
          >
            <div className="flex flex-wrap gap-4 items-end">
              <div className="flex-1 min-w-[200px]">
                <label className="block text-[9px] font-bold text-white/30 uppercase tracking-widest mb-1.5 flex items-center gap-2">
                  <Search size={10} />
                  Keyword / Regex
                </label>
                <input
                  type="text"
                  value={filterKeyword}
                  onChange={(e) => setFilterKeyword(e.target.value)}
                  placeholder="Filter logs..."
                  className="w-full bg-[#0d0d0d] border border-white/10 rounded-sm px-3 py-1.5 text-[11px] font-mono text-white/80 focus:outline-none focus:border-[#ff6b35]/40 transition-colors placeholder:text-white/10"
                />
              </div>

              <div className="flex gap-2">
                {(["log", "info", "warn", "error"] as LogLevel[]).map((level) => (
                  <button
                    key={level}
                    onClick={() => toggleLevel(level)}
                    className={`px-2.5 py-1.5 rounded-sm text-[9px] font-bold uppercase border transition-all ${
                      filterLevels.includes(level)
                        ? level === "error"
                          ? "bg-red-500/10 border-red-500/30 text-red-400"
                          : level === "warn"
                            ? "bg-yellow-500/10 border-yellow-500/30 text-yellow-400"
                            : level === "info"
                              ? "bg-blue-500/10 border-blue-500/30 text-blue-400"
                              : "bg-white/10 border-white/30 text-white"
                        : "bg-white/2 border-white/5 text-white/20 hover:text-white/40"
                    }`}
                  >
                    {level}
                  </button>
                ))}
              </div>
            </div>

            <div className="flex flex-wrap gap-4">
              <div className="flex-1 min-w-[150px]">
                <label className="block text-[9px] font-bold text-white/30 uppercase tracking-widest mb-1.5 flex items-center gap-2">
                  <Calendar size={10} />
                  Date Start
                </label>
                <input
                  type="datetime-local"
                  value={dateStart}
                  onChange={(e) => setDateStart(e.target.value)}
                  className="w-full bg-[#0d0d0d] border border-white/10 rounded-sm px-3 py-1.5 text-[11px] font-mono text-white/80 focus:outline-none focus:border-[#ff6b35]/40 transition-colors [color-scheme:dark]"
                />
              </div>
              <div className="flex-1 min-w-[150px]">
                <label className="block text-[9px] font-bold text-white/30 uppercase tracking-widest mb-1.5 flex items-center gap-2">
                  <Clock size={10} />
                  Date End
                </label>
                <input
                  type="datetime-local"
                  value={dateEnd}
                  onChange={(e) => setDateEnd(e.target.value)}
                  className="w-full bg-[#0d0d0d] border border-white/10 rounded-sm px-3 py-1.5 text-[11px] font-mono text-white/80 focus:outline-none focus:border-[#ff6b35]/40 transition-colors [color-scheme:dark]"
                />
              </div>
              <div className="flex items-end pb-0.5">
                <button
                  onClick={() => {
                    setFilterKeyword("");
                    setFilterLevels(["log", "info", "warn", "error"]);
                    setDateStart("");
                    setDateEnd("");
                  }}
                  className="px-3 py-1.5 text-[9px] font-bold uppercase text-white/30 hover:text-white transition-colors"
                >
                  Reset_Filters
                </button>
              </div>
            </div>
          </motion.div>
        )}
      </AnimatePresence>

      <div className="flex-1 overflow-y-auto overflow-x-hidden debug-scrollbar font-mono text-[11px] space-y-1 pr-1">
        {filteredLogs.length === 0 ? (
          <div className="flex flex-col items-center justify-center h-full text-white/10 py-20 grayscale opacity-50">
            <Monitor size={48} className="mb-4" />
            <p className="uppercase tracking-[0.3em] text-[10px] font-bold">
              {logs.length > 0 ? "No_Matches_Found" : "No_Logs_Recorded"}
            </p>
          </div>
        ) : (
          <div>
            {[...filteredLogs].reverse().map((log) => (
              <div
                key={log.id}
                className="flex gap-3 py-1 px-2 border-b border-white/[0.03] hover:bg-white/[0.02] group"
              >
                <span className="text-white/20 select-none w-24 flex-shrink-0 text-[10px] whitespace-nowrap">
                  {new Date(log.timestamp)
                    .toLocaleString([], {
                      month: "2-digit",
                      day: "2-digit",
                      hour: "2-digit",
                      minute: "2-digit",
                      second: "2-digit",
                      hour12: false,
                    })
                    .replace(",", "")}
                </span>
                <span
                  className={`uppercase font-bold text-[9px] w-12 flex-shrink-0 mt-0.5 ${
                    log.level === "error"
                      ? "text-red-400"
                      : log.level === "warn"
                        ? "text-yellow-400"
                        : log.level === "info"
                          ? "text-blue-400"
                          : "text-white/40"
                  }`}
                >
                  [{log.level}]
                </span>
                <div className="flex-1 flex flex-wrap items-start gap-x-2 min-w-0">
                  {log.args.map((arg, i) => {
                    if (typeof arg === "string") {
                      return (
                        <span
                          key={i}
                          className={`whitespace-pre-wrap break-all ${
                            log.level === "error"
                              ? "text-red-300/90"
                              : log.level === "warn"
                                ? "text-yellow-200/80"
                                : "text-gray-300"
                          }`}
                        >
                          {arg}
                        </span>
                      );
                    }
                    if (arg === null || arg === undefined || typeof arg !== "object") {
                      return (
                        <span key={i} className="text-white/40 tabular-nums">
                          {String(arg)}
                        </span>
                      );
                    }
                    return (
                      <div
                        key={i}
                        className="w-full mt-1 mb-2 p-2 bg-white/[0.02] border border-white/10 rounded-sm overflow-x-auto debug-scrollbar"
                      >
                        <JsonNode value={arg} depth={1} isWrapping={isWrapping} />
                      </div>
                    );
                  })}
                </div>
              </div>
            ))}
          </div>
        )}
        {logs.length > 0 && (
          <div className="pt-8 text-center opacity-10 uppercase tracking-[0.4em] font-bold text-[9px]">
            [ START_OF_LOG_STREAM ]
          </div>
        )}
      </div>
    </div>
  );
};
