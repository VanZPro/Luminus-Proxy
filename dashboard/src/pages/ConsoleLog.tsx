import { useEffect, useRef, useState } from "react";
import { Card, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { useWsEvent, useWsStatus } from "@/hooks/useWebSocket";
import { fetchApi } from "@/lib/api";
import { Terminal, Trash2, Play, Square, Download } from "lucide-react";

interface LogEntry {
  id: string;
  timestamp: string;
  level: "info" | "error" | "warn";
  message: string;
}

export default function ConsoleLog() {
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [paused, setPaused] = useState(false);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const wsStatus = useWsStatus();
  const bottomRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let cancelled = false;
    async function loadHistory() {
      try {
        setLoading(true);
        const response = await fetchApi<{ data?: any[] }>("/api/stats/requests?limit=200");
        if (cancelled) return;
        const history = (response.data || []).reverse().map((row: any, index: number) => ({
          id: `history-${row.id ?? index}`,
          timestamp: row.createdAt ? new Date(row.createdAt).toLocaleTimeString() : new Date().toLocaleTimeString(),
          level: row.status === "error" ? "error" : "info",
          message: row.status === "error"
            ? `ERROR: /v1/chat/completions | Model: ${row.model || "unknown"} | Error: ${row.errorMessage || "Unknown error"}`
            : `REQUEST: /v1/chat/completions | Model: ${row.model || "unknown"} | Status: ${row.status || "success"} ${row.durationMs ? `[${row.durationMs}ms]` : ""}`,
        } satisfies LogEntry));
        setLogs(history);
        setLoadError(null);
      } catch (error) {
        if (!cancelled) setLoadError(error instanceof Error ? error.message : "Failed to load request history");
      } finally {
        if (!cancelled) setLoading(false);
      }
    }
    void loadHistory();
    return () => { cancelled = true; };
  }, []);

  // Auto scroll to bottom
  useEffect(() => {
    if (!paused) {
      bottomRef.current?.scrollIntoView({ behavior: "smooth" });
    }
  }, [logs, paused]);

  // Capture WS events
  useWsEvent(["request_log", "request_error"], (msg) => {
    if (paused) return;

    const level = msg.type === "request_error" ? "error" : "info";
    const rawData = msg.data;

    let logText = "";
    if (typeof rawData === "string") {
      logText = rawData;
    } else if (rawData && typeof rawData === "object") {
      const model = rawData.model || "unknown";
      const latency = rawData.latency_ms ? `[${rawData.latency_ms}ms]` : "";
      const status = rawData.status || "";
      const path = rawData.path || "/v1/chat/completions";

      if (level === "error") {
        logText = `ERROR: ${path} | Model: ${model} | Error: ${rawData.error || "Unknown error"}`;
      } else {
        logText = `REQUEST: ${path} | Model: ${model} | Status: ${status} ${latency}`;
      }
    }

    const newEntry: LogEntry = {
      id: Math.random().toString(36).substr(2, 9),
      timestamp: new Date().toLocaleTimeString(),
      level,
      message: logText,
    };

    // Mirror to the browser developer console for debugging
    const tag = `[Luminus:${msg.type}]`;
    if (level === "error") {
      console.error(tag, rawData);
    } else {
      console.log(tag, rawData);
    }

    setLogs((prev) => [...prev.slice(-200), newEntry]); // Keep last 200 logs
  });

  function clearLogs() {
    setLogs([]);
  }

  function downloadLogs() {
    const text = logs.map((l) => `[${l.timestamp}] [${l.level.toUpperCase()}] ${l.message}`).join("\n");
    const blob = new Blob([text], { type: "text/plain" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `luminus-console-${new Date().toISOString().slice(0,10)}.log`;
    a.click();
    URL.revokeObjectURL(url);
  }

  return (
    <div className="space-y-4">
      {/* Header */}
      <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <h1 className="text-2xl font-bold text-[var(--foreground)] flex items-center gap-2">
            <Terminal className="w-5 h-5 text-[var(--primary)]" /> Console Log
          </h1>
          <p className="text-sm text-[var(--muted-foreground)] mt-1">
            Real-time proxy requests and server event streaming
          </p>
        </div>
        <div className="flex items-center gap-2">
          <Button
            variant="outline"
            size="sm"
            onClick={() => setPaused(!paused)}
            className="gap-2"
          >
            {paused ? (
              <><Play className="w-4 h-4 text-[var(--success)]" /> Resume</>
            ) : (
              <><Square className="w-4 h-4 text-[var(--warning)]" /> Pause</>
            )}
          </Button>
          <Button
            variant="outline"
            size="sm"
            onClick={downloadLogs}
            disabled={logs.length === 0}
            className="gap-2"
          >
            <Download className="w-4 h-4" /> Export
          </Button>
          <Button
            variant="outline"
            size="sm"
            onClick={clearLogs}
            disabled={logs.length === 0}
            className="gap-2 text-[var(--error)]"
          >
            <Trash2 className="w-4 h-4" /> Clear
          </Button>
        </div>
      </div>

      {/* Terminal View */}
      <Card className="border-[var(--border)] bg-[#0d0e11]">
        <CardContent className="p-4">
          <div className="font-mono text-xs text-[#d1d5db] h-[calc(100vh-220px)] min-h-[300px] overflow-y-auto space-y-1.5 scrollbar-thin">
            <div className="text-[var(--muted-foreground)] pb-2 border-b border-[var(--border)]/20">
              -- LUMINUS CONSOLE LOG STREAM STARTED -- WS: {wsStatus.toUpperCase()} --
            </div>
            {loadError && (
              <div className="text-[var(--error)] py-2">History unavailable: {loadError}</div>
            )}
            {logs.map((log) => (
              <div key={log.id} className="flex gap-2 hover:bg-[#1a1b20]/50 p-0.5 rounded transition-colors">
                <span className="text-[var(--muted-foreground)] shrink-0 select-none">
                  [{log.timestamp}]
                </span>
                <span
                  className={`font-semibold shrink-0 select-none ${
                    log.level === "error"
                      ? "text-[var(--error)]"
                      : log.level === "warn"
                      ? "text-[var(--warning)]"
                      : "text-[var(--info)]"
                  }`}
                >
                  [{log.level.toUpperCase()}]
                </span>
                <span className="break-all">{log.message}</span>
              </div>
            ))}
            {loading && logs.length === 0 && (
              <div className="text-[var(--muted-foreground)] italic pt-4 text-center">
                Loading recent API calls...
              </div>
            )}
            {!loading && logs.length === 0 && !loadError && (
              <div className="text-[var(--muted-foreground)] italic pt-4 text-center">
                No request history yet — live events will appear here.
              </div>
            )}
            <div ref={bottomRef} />
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
